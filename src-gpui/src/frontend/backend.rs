use super::*;
use gpui_kit::component::WindowExt;
impl Desktop {
    pub(in crate::frontend) fn request(
        &mut self,
        command: &str,
        args: Value,
        cache: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if cache.is_some_and(|key| {
            self.pending
                .values()
                .any(|p| p.cache.as_deref() == Some(key))
        }) {
            return;
        }
        if self.options.fixture.is_some() {
            if let Some(key) = cache
                && self.data.contains_key(key)
                && !command.starts_with("set_")
            {
                return;
            }
            if !matches!(
                command,
                "get_settings"
                    | "app_info"
                    | "set_interval"
                    | "set_paused"
                    | "refresh"
                    | "update_status"
                    | "set_theme"
                    | "set_language"
            ) {
                return;
            }
        }
        self.next_request += 1;
        let id = self.next_request;
        self.pending.insert(
            id,
            Pending {
                command: command.into(),
                cache: cache.map(str::to_owned),
                args: args.clone(),
            },
        );
        let main = matches!(
            command,
            "get_settings"
                | "app_info"
                | "set_theme"
                | "set_language"
                | "set_interval"
                | "set_paused"
                | "refresh"
                | "set_hotkey"
                | "set_hotkey_enabled"
                | "suspend_hotkey"
                | "resume_hotkey"
                | "minimize_main"
                | "set_window_motion"
                | "quit"
                | "page_painted"
                | "task_manager_ready"
        );
        if main {
            let result = crate::dispatch::invoke(&self.host, command, args);
            self.complete(id, result, window, cx);
        } else {
            let host = self.host.clone();
            let command = command.to_owned();
            std::thread::spawn(move || {
                let result = crate::dispatch::invoke(&host, &command, args);
                let _ = host.emit(
                    "command-response",
                    json!({"id":id,"ok":result.is_ok(),"value":result.unwrap_or_else(|e|e)}),
                );
            });
        }
    }
    pub(in crate::frontend) fn complete(
        &mut self,
        id: u64,
        result: Result<Value, Value>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(pending) = self.pending.remove(&id) else {
            return;
        };
        if pending.command == "history_series" && pending.args["range"] != self.history_range {
            self.load_history(window, cx);
            return;
        }
        if pending.command == "history_moment"
            && pending.args["cursor"] != json!(self.history_cursor)
        {
            self.load_history(window, cx);
            return;
        }
        match result {
            Ok(value) => {
                if pending.command == "history_series" {
                    self.data.insert(
                        "historyWindow".into(),
                        json!({"from":pending.args["from"],"to":pending.args["to"]}),
                    );
                }
                if let Some(key) = pending.cache {
                    self.data.insert(key, value.clone());
                }
                if pending.command.starts_with("set_") && value.get("closeToTray").is_some() {
                    self.data.insert("settings".into(), value.clone());
                }
                self.command_completed(&pending.command, &value, window, cx);
                self.rebuild_rows(
                    self.pointer_in_table || self.modal.is_some() || self.popup.is_some(),
                    cx,
                );
            }
            Err(error) => {
                let message = error
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| self.action_error(&pending.command, &pending.args, &error));
                if let Some(modal) = &mut self.modal {
                    modal.error = Some(message.clone());
                }
                self.toast = Some((message, Instant::now()));
            }
        }
        if self.modal.is_some() {
            window.refresh();
        }
        cx.notify();
    }
    pub(in crate::frontend) fn notifications(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for event in self.host.drain() {
            match event.name.as_str() {
                "command-response" => self.complete(
                    event.value["id"].as_u64().unwrap_or(0),
                    if event.value["ok"].as_bool().unwrap_or(false) {
                        Ok(event.value["value"].clone())
                    } else {
                        Err(event.value["value"].clone())
                    },
                    window,
                    cx,
                ),
                "update-status" => {
                    self.data.insert("updates".into(), event.value);
                }
                "argus-theme" => {
                    self.set_theme(event.value.as_bool().unwrap_or(false), window, cx);
                }
                "argus-language" => self.change_placeholder(window, cx),
                "argus-blur" => self.set_control_paused(false, window, cx),
                "argus-show-update" => self.open_dialog("update", json!({}), window, cx),
                "argus-taskmgr-ready" => {
                    self.request("task_manager_ready", json!({}), None, window, cx)
                }
                "test-control" => self.test_control(&event.value, window, cx),
                _ => {}
            }
        }
        if self.host.exit_code().is_some() {
            cx.quit();
            return;
        }
        if self.options.fixture.is_none() {
            let latest = self.host.state::<crate::Latest>().0.read().unwrap().clone();
            if let Some(snapshot) = latest
                && snapshot.seq != self.sequence
            {
                self.sequence = snapshot.seq;
                if !self.options.test {
                    crate::shell::set_tray_tooltip(
                        &self.host,
                        &crate::shell::tray_tooltip(&tr!(
                            "tray.tooltip",
                            cpu = format!("{:.0}", snapshot.system.cpu),
                            memory = format!(
                                "{:.0}",
                                snapshot.system.mem_used as f64
                                    / snapshot.system.mem_total.max(1) as f64
                                    * 100.
                            )
                        )),
                    );
                }
                self.accept_snapshot(
                    serde_json::to_value(snapshot.as_ref()).expect("snapshot model"),
                    cx,
                );
            }
        }
        if !self.snapshot.is_null() && !self.first_frame {
            self.first_frame = true;
            if self.host.state::<AppState>().task_manager_start {
                crate::commands::task_manager_ready(self.host.clone());
            }
        }
        self.check_welcome(window, cx);
        cx.notify();
    }
    pub(in crate::frontend) fn accept_snapshot(&mut self, snapshot: Value, cx: &mut Context<Self>) {
        self.chart_clock.borrow_mut().accept(
            number(&snapshot, "timestamp"),
            self.interval as f64,
            Instant::now(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs_f64()
                * 1000.,
        );
        self.sequence = snapshot["seq"].as_u64().unwrap_or(self.sequence + 1);
        self.timeline.push(&snapshot, self.interval as f64);
        self.snapshot = snapshot;
        self.rebuild_rows(
            self.pointer_in_table || self.popup.is_some() || self.modal.is_some(),
            cx,
        );
    }
    pub(in crate::frontend) fn poll_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.page.as_str() {
            "processes" => {
                self.request("desktop_shell", json!({}), Some("desktopShell"), window, cx);
                self.request("list_services", json!({}), Some("services"), window, cx);
            }
            "services" => self.request("list_services", json!({}), Some("services"), window, cx),
            "startup" => self.request("list_startup", json!({}), Some("startup"), window, cx),
            "network" => self.request(
                "list_connections",
                json!({}),
                Some("connections"),
                window,
                cx,
            ),
            "settings" => {
                self.request("history_usage", json!({}), Some("historyUsage"), window, cx);
                self.request(
                    "task_manager_status",
                    json!({}),
                    Some("taskmgr"),
                    window,
                    cx,
                );
                self.request(
                    "privileged_service_status",
                    json!({}),
                    Some("privilege"),
                    window,
                    cx,
                );
                self.request("update_status", json!({}), Some("updates"), window, cx);
                self.request("rule_stats", json!({}), Some("ruleStats"), window, cx);
            }
            "history" => self.load_history(window, cx),
            _ => {}
        }
    }
    pub(in crate::frontend) fn change_placeholder(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = match self.page.as_str() {
            "services" => "services.search",
            "startup" => "startup.search",
            "network" => "connections.search",
            _ => "shell.search.processes",
        };
        self.search.update(cx, |state, cx| {
            state.set_placeholder(label(key), window, cx)
        });
    }
    pub(in crate::frontend) fn action_error(
        &self,
        command: &str,
        args: &Value,
        error: &Value,
    ) -> String {
        if let Some(message) = error["message"].as_str() {
            return message.into();
        }
        let kind = error
            .as_str()
            .or_else(|| error.get("kind").and_then(Value::as_str))
            .unwrap_or("failed");
        if command.contains("hotkey") {
            let shortcut = text(error, "shortcut");
            let issue = text(error, "issue");
            let reason = argus_core::i18n::format(
                &format!("settings.hotkey.issue.{issue}"),
                &[("shortcut", &shortcut), ("key", &shortcut)],
            );
            let key = format!("settings.hotkey.error.{kind}");
            let message = argus_core::i18n::format(
                &key,
                &[
                    ("shortcut", &shortcut),
                    ("reason", &reason),
                    ("code", &error["code"]),
                ],
            );
            if message != key {
                return message;
            }
        }
        let key = format!(
            "{}.{kind}",
            if command == "run_task" {
                "tasks.error"
            } else {
                "actions.error"
            }
        );
        let name = args["pid"]
            .as_u64()
            .and_then(|pid| {
                values(&self.snapshot["processes"])
                    .iter()
                    .find(|p| p["pid"].as_u64() == Some(pid))
                    .map(model::title)
            })
            .unwrap_or_else(|| text(args, "name").into());
        let result = argus_core::i18n::format(&key, &[("name", &name)]);
        if result == key {
            error
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| tr!("app.invalidRequest"))
        } else {
            result
        }
    }
    pub(in crate::frontend) fn act(
        &mut self,
        action: Action,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            Action::Page(page) => {
                if !PAGES.iter().any(|p| p.0 == page) {
                    return;
                }
                self.sorts
                    .insert(self.page.clone(), (self.sort.clone(), self.descending));
                self.queries.insert(self.page.clone(), self.query.clone());
                self.page = page;
                let saved = self.sorts.get(&self.page).cloned().unwrap_or_else(|| {
                    if self.page == "processes" {
                        let sort: Value = self.prefs.get("sort", json!({"key":"cpu","desc":true}));
                        (text(&sort, "key").into(), boolean(&sort, "desc"))
                    } else {
                        ("name".into(), false)
                    }
                });
                self.sort = saved.0;
                self.descending = saved.1;
                self.table = self.tables.get(&self.page).map(|table| table.0.clone());
                self.table_frame = self.tables.get(&self.page).map(|table| table.1.clone());
                self.prefs.set("page", &self.page);
                self.query = self.queries.get(&self.page).cloned().unwrap_or_default();
                self.search
                    .update(cx, |s, cx| s.set_value(self.query.clone(), window, cx));
                self.change_placeholder(window, cx);
                self.horizontal = 0.;
                self.popup = None;
                self.rebuild_rows(false, cx);
                self.list.scroll_to(ListOffset {
                    item_ix: 0,
                    offset_in_item: px(0.),
                });
                self.last_poll = Instant::now();
                self.poll_page(window, cx);
            }
            Action::View(view) => {
                self.view = view;
                self.prefs.set("view", &self.view);
                self.rebuild_rows(false, cx);
            }
            Action::Select(key) => {
                self.selected.insert(self.page.clone(), key.clone());
                if self.page == "processes"
                    && let Some(row) = self.rows.iter().find(|r| r.key == key)
                {
                    let raw = row.raw.clone();
                    self.request(
                        "process_details",
                        json!({"pid":raw["pid"],"path":raw["path"]}),
                        Some(&format!("details:{key}")),
                        window,
                        cx,
                    );
                }
            }
            Action::Fold(key) => {
                if let Some(section) = key.strip_prefix("s:") {
                    if !self.sections.remove(section) {
                        self.sections.insert(section.into());
                    }
                    self.prefs.set(
                        "collapsedSections",
                        self.sections.iter().collect::<Vec<_>>(),
                    );
                } else {
                    let set = if self.view == "tree" {
                        &mut self.collapsed
                    } else {
                        &mut self.expanded
                    };
                    if !set.remove(&key) {
                        set.insert(key);
                    }
                }
                self.rebuild_rows(false, cx);
            }
            Action::Details => {
                self.details_open = !self.details_open;
                self.prefs.set("detailsPanel", self.details_open);
            }
            Action::Pause => {
                self.paused = !self.paused;
                self.chart_clock
                    .borrow_mut()
                    .set_paused(self.paused || self.control_paused);
                self.request(
                    "set_paused",
                    json!({"paused":self.paused||self.control_paused}),
                    None,
                    window,
                    cx,
                );
            }
            Action::Refresh => {
                self.request("refresh", json!({}), None, window, cx);
                self.poll_page(window, cx);
            }
            Action::Interval(ms) => {
                self.interval = ms;
                self.prefs.set("interval", ms);
                self.request("set_interval", json!({"ms":ms}), None, window, cx);
            }
            Action::Device(device) => {
                self.device = device;
                self.prefs.set("perfDevice", &self.device);
                self.page = "performance".into();
            }
            Action::CpuMode(mode) => {
                if mode == "details" {
                    self.cpu_page = "details".into();
                } else {
                    self.cpu_page = "overview".into();
                    self.cpu_mode = mode;
                    self.prefs.set("cpuMode", &self.cpu_mode);
                }
                self.prefs.set("cpuPage", &self.cpu_page);
            }
            Action::Invoke(command, args) => self.request(&command, args, None, window, cx),
            Action::Dialog(kind, args) => self.open_dialog(&kind, args, window, cx),
            Action::Popup(items) => {
                self.open_popup(items, window, cx);
            }
            Action::Preference(key, value) => self.preference(&key, value, window, cx),
            Action::Copy(value) => cx.write_to_clipboard(ClipboardItem::new_string(value)),
            Action::CloseOverlay => {
                window.close_dialog(cx);
                self.popup = None;
                self.modal = None;
                self.request("resume_hotkey", json!({}), Some("settings"), window, cx);
                self.focus.focus(window, cx);
            }
            Action::Confirm => self.confirm_dialog(window, cx),
        }
        cx.notify();
    }
    pub(in crate::frontend) fn key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = event.keystroke.key.as_str();
        let m = event.keystroke.modifiers;
        if self.page == "history" && self.modal.is_none() && self.popup.is_none() {
            if key == "escape" && self.history_cursor.is_some() {
                self.history_cursor = None;
                self.load_history(window, cx);
                cx.notify();
                return;
            }
            if matches!(key, "left" | "right" | "home" | "end") {
                let points = self
                    .data
                    .get("historySeries")
                    .map(values)
                    .unwrap_or_default();
                if points.is_empty() {
                    return;
                }
                let current = self
                    .history_cursor
                    .and_then(|time| {
                        points
                            .iter()
                            .enumerate()
                            .min_by_key(|(_, p)| {
                                (p["ts"].as_i64().unwrap_or(0) - time).unsigned_abs()
                            })
                            .map(|(i, _)| i)
                    })
                    .unwrap_or(points.len() - 1);
                let step = if m.shift {
                    (points.len() / 20).max(1)
                } else {
                    1
                };
                let index = match key {
                    "left" => current.saturating_sub(step),
                    "right" => (current + step).min(points.len() - 1),
                    "home" => 0,
                    _ => points.len() - 1,
                };
                self.history_cursor = points[index]["ts"].as_i64();
                self.load_history(window, cx);
                cx.notify();
                cx.stop_propagation();
                return;
            }
        }
        if key == "escape" {
            if self.popup.is_some() || self.modal.is_some() {
                self.act(Action::CloseOverlay, window, cx);
            } else if !self.query.is_empty() {
                self.search.update(cx, |s, cx| s.set_value("", window, cx));
                self.query.clear();
                self.rebuild_rows(false, cx);
                cx.notify();
            } else {
                self.selected.remove(&self.page);
                cx.notify();
            }
            return;
        }
        if self.modal.is_some() {
            if key == "enter" {
                self.act(Action::Confirm, window, cx);
            }
            return;
        }
        if m.control {
            if let Ok(index) = key.parse::<usize>()
                && index > 0
                && index <= PAGES.len()
            {
                self.act(Action::Page(PAGES[index - 1].0.into()), window, cx);
                cx.stop_propagation();
                return;
            }
            if key == "f" {
                self.search.update(cx, |state, cx| state.focus(window, cx));
                cx.stop_propagation();
                return;
            }
        }
        if key == "f5" {
            self.act(Action::Refresh, window, cx);
            return;
        }
        if self.search.read(cx).focus_handle(cx).is_focused(window) {
            return;
        }
        if matches!(key, "up" | "down" | "home" | "end") {
            let current = self
                .selected
                .get(&self.page)
                .and_then(|key| self.rows.iter().position(|r| &r.key == key))
                .unwrap_or(0);
            let index = match key {
                "up" => current.saturating_sub(1),
                "down" => (current + 1).min(self.rows.len().saturating_sub(1)),
                "end" => self.rows.len().saturating_sub(1),
                _ => 0,
            };
            if let Some(row) = self.rows.get(index) {
                self.act(Action::Select(row.key.clone()), window, cx);
                self.list.scroll_to_reveal_item(index);
            }
            cx.stop_propagation();
        } else if key == "enter"
            && let Some(key) = self.selected.get(&self.page)
        {
            if self.page == "processes" {
                self.act(Action::Details, window, cx);
            } else {
                self.act(Action::Fold(key.clone()), window, cx);
            }
        } else if (key == "delete" || (m.alt && key == "e")) && self.page == "processes" {
            self.open_selected_action("end", window, cx);
            if m.shift {
                self.confirm_dialog(window, cx);
            }
        } else if matches!(
            self.page.as_str(),
            "processes" | "services" | "startup" | "network"
        ) && !m.control
            && !m.alt
            && !m.platform
            && key.chars().count() == 1
        {
            self.search.update(cx, |state, cx| {
                state.set_value(key.to_owned(), window, cx);
                state.focus(window, cx);
            });
            self.query = key.into();
            self.rebuild_rows(false, cx);
            cx.notify();
        }
    }
    pub(in crate::frontend) fn modifiers_changed(
        &mut self,
        event: &ModifiersChangedEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let paused = event.modifiers.control
            && matches!(self.page.as_str(), "processes" | "performance")
            && self.modal.is_none()
            && !self.search.read(cx).focus_handle(cx).is_focused(window);
        self.set_control_paused(paused, window, cx);
    }
    fn set_control_paused(&mut self, paused: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.control_paused != paused {
            self.control_paused = paused;
            self.chart_clock
                .borrow_mut()
                .set_paused(self.paused || paused);
            self.request(
                "set_paused",
                json!({"paused":self.paused||paused}),
                None,
                window,
                cx,
            );
        }
    }
    pub(in crate::frontend) fn test_control(
        &mut self,
        control: &Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if control["exit"].as_bool() == Some(true) {
            self.host.exit(0);
            return;
        }
        if let Some(section) = control["settingsSection"].as_str() {
            self.settings_section = section.into();
        }
        if let Some(mode) = control["motion"].as_str() {
            self.preference("motion.chartMode", json!(mode), window, cx);
        }
        if let Some(language) = control["language"].as_str() {
            self.request(
                "set_language",
                json!({"language":language}),
                Some("settings"),
                window,
                cx,
            );
        }
        if let Some(page) = control["page"].as_str() {
            self.act(Action::Page(page.into()), window, cx);
        }
        if let Some(query) = control["query"].as_str() {
            self.query = query.into();
            self.search
                .update(cx, |s, cx| s.set_value(query.to_owned(), window, cx));
            self.rebuild_rows(false, cx);
        }
        if let Some(index) = control["selectRow"].as_u64()
            && let Some(row) = self.rows.get(index as usize)
        {
            self.act(Action::Select(row.key.clone()), window, cx);
        }
        if control["dismiss"].as_bool() == Some(true) {
            self.act(Action::CloseOverlay, window, cx);
        }
        if let Some(view) = control["view"].as_str() {
            self.act(Action::View(view.into()), window, cx);
        }
        if let Some(device) = control["device"].as_str() {
            self.act(Action::Device(device.into()), window, cx);
        }
        if let Some(mode) = control["cpuMode"].as_str() {
            self.act(Action::CpuMode(mode.into()), window, cx);
        }
        if let Some(value) = control["details"].as_bool() {
            self.details_open = value;
        }
        if let Some(dialog) = control["dialog"].as_str() {
            self.open_dialog(dialog, control["args"].clone(), window, cx);
        }
        if let Some(theme) = control["theme"].as_str() {
            self.request(
                "set_theme",
                json!({"theme":theme}),
                Some("settings"),
                window,
                cx,
            );
        }
        cx.notify();
    }
}
