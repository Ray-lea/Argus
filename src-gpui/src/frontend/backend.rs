use super::*;
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
                // Polling keeps external state current; identical read results do
                // not invalidate the process table or the rest of the desktop.
                if matches!(
                    pending.command.as_str(),
                    "desktop_shell"
                        | "list_services"
                        | "list_startup"
                        | "list_connections"
                        | "history_usage"
                        | "task_manager_status"
                        | "privileged_service_status"
                        | "update_status"
                        | "rule_stats"
                ) && pending
                    .cache
                    .as_ref()
                    .is_some_and(|key| self.data.get(key) == Some(&value))
                {
                    return;
                }
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
                self.command_failed(&pending.command, &pending.args, &error, message, window, cx);
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
        let mut changed = false;
        for event in self.host.drain() {
            let started = Instant::now();
            // What ran, for the frame trace only.
            let what = gpui_kit::gpui::profiler::phase_trace_enabled()
                .then(|| match event.name.as_str() {
                    "command-response" => event.value["id"]
                        .as_u64()
                        .and_then(|id| self.pending.get(&id))
                        .map(|pending| pending.command.clone()),
                    "test-control" => event.value.as_object().map(|control| {
                        control
                            .keys()
                            .filter(|key| *key != "pointerOutside")
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(",")
                    }),
                    _ => None,
                })
                .flatten();
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
                    changed |= self.data.get("updates") != Some(&event.value);
                    self.data.insert("updates".into(), event.value);
                }
                "argus-theme" => {
                    self.set_theme(event.value.as_bool().unwrap_or(false), window, cx);
                }
                "argus-language" => {
                    self.change_placeholder(window, cx);
                    changed = true;
                }
                "argus-blur" => {
                    self.set_control_paused(false, window, cx);
                    self.hotkey_blur(window, cx);
                }
                "argus-restored" => self.restored(&event.value),
                "argus-hotkey-changed" => self.hotkey_changed(window, cx),
                "argus-show-update" => self.open_dialog("update", json!({}), window, cx),
                "argus-taskmgr-ready" => {
                    self.request("task_manager_ready", json!({}), None, window, cx)
                }
                "test-control" => self.test_control(&event.value, window, cx),
                _ => {}
            }
            super::trace_work(&event.name, what.as_deref(), started);
        }
        if self.host.exit_code().is_some() {
            cx.quit();
            return;
        }
        if self.options.fixture.is_none() {
            let latest = self.host.state::<crate::Latest>().0.read().unwrap().clone();
            if let Some(snapshot) = latest
                && snapshot.seq != self.sequence
                && !self.snapshot_pending
            {
                self.snapshot_pending = true;
                let epoch = self.rows_epoch;
                let locale = argus_core::i18n::locale();
                let processes = self.page == "processes";
                let options = (
                    self.view.clone(),
                    self.query.clone(),
                    self.sort.clone(),
                    self.descending,
                    self.expanded.clone(),
                    self.collapsed.clone(),
                    self.sections.clone(),
                    self.hosted_services(),
                );
                let task = cx.background_executor().spawn(async move {
                    let start = Instant::now();
                    let model = serde_json::to_value(snapshot.as_ref()).expect("snapshot model");
                    let serialize_us = start.elapsed().as_micros() as u64;
                    let start = Instant::now();
                    let rows = processes.then(|| {
                        model::process_rows(
                            values(&model["processes"]),
                            model::ProcessOptions {
                                mode: &options.0,
                                query: &options.1,
                                sort: &options.2,
                                descending: options.3,
                                expanded: &options.4,
                                collapsed: &options.5,
                                sections: &options.6,
                                hosted: &options.7,
                            },
                        )
                    });
                    (
                        model,
                        rows,
                        serialize_us,
                        start.elapsed().as_micros() as u64,
                    )
                });
                cx.spawn_in(window, async move |this, cx| {
                    let (model, rows, serialize_us, group_us) = task.await;
                    let _ = this.update_in(cx, |this, window, cx| {
                        let started = Instant::now();
                        this.snapshot_pending = false;
                        this.work.serialize_us += serialize_us;
                        this.work.background_group_us += group_us;
                        let rows =
                            if epoch == this.rows_epoch && locale == argus_core::i18n::locale() {
                                rows
                            } else {
                                None
                            };
                        this.accept_snapshot_prepared(model, rows, cx);
                        super::trace_work("snapshot", None, started);
                        // Pick up the newest source sequence without accumulating a UI queue.
                        this.notifications(window, cx);
                        cx.notify();
                    });
                })
                .detach();
            }
        }
        if !self.snapshot.is_null() && !self.first_frame {
            self.first_frame = true;
            self.first_frame_at = Some(Instant::now());
            // An administrator start counts as succeeded once data is on screen.
            crate::admin::painted();
            if self.host.state::<AppState>().task_manager_start {
                crate::commands::task_manager_ready(self.host.clone());
            }
        }
        self.check_welcome(window, cx);
        if changed {
            cx.notify();
        }
    }
    pub(in crate::frontend) fn accept_snapshot(&mut self, snapshot: Value, cx: &mut Context<Self>) {
        self.accept_snapshot_prepared(snapshot, None, cx);
    }
    fn accept_snapshot_prepared(
        &mut self,
        snapshot: Value,
        rows: Option<Vec<Row>>,
        cx: &mut Context<Self>,
    ) {
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
        let start = Instant::now();
        self.timeline.push(&snapshot, self.interval as f64);
        self.work.timeline_us += start.elapsed().as_micros() as u64;
        let previous = std::mem::replace(&mut self.snapshot, snapshot);
        // New processes are tinted for a few samples, as in the original list.
        let start_up = previous.is_null();
        let seen = std::mem::take(&mut self.first_seen);
        self.first_seen = values(&self.snapshot["processes"])
            .iter()
            .map(|p| {
                let id = model::identity(p);
                let first = if start_up {
                    0
                } else {
                    seen.get(&id).copied().unwrap_or(self.sequence)
                };
                (id, first)
            })
            .collect();
        if !previous.is_null() {
            cx.background_executor()
                .spawn(async move { drop(previous) })
                .detach();
        }
        if matches!(self.page.as_str(), "processes" | "network") {
            self.rebuild_rows_prepared(
                self.pointer_in_table || self.popup.is_some() || self.modal.is_some(),
                rows,
                cx,
            );
        }
    }
    /// Whether a row's processes all started within the last three samples.
    pub(in crate::frontend) fn is_fresh(&self, row: &Row) -> bool {
        row.section.is_none()
            && !row.members.is_empty()
            && row.members.iter().all(|member| {
                self.first_seen
                    .get(&model::identity(member))
                    .is_some_and(|first| *first != 0 && self.sequence.saturating_sub(*first) < 3)
            })
    }
    /// How often the visible page re-reads its list, as in the original pages.
    pub(in crate::frontend) fn poll_interval(&self) -> Duration {
        Duration::from_millis(match self.page.as_str() {
            "services" | "processes" => 3000,
            "startup" => 10000,
            _ => 2000,
        })
    }
    /// Reads the lists of the other table pages once, a second after the first
    /// snapshot, so that the first visit shows them at once instead of waiting
    /// for the read (cold, the service list alone took over a second). Each page
    /// still re-reads its list when shown; the process page already reads the
    /// services.
    pub(in crate::frontend) fn prefetch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.prefetched
            || self
                .first_frame_at
                .is_none_or(|at| at.elapsed() < Duration::from_secs(1))
        {
            return;
        }
        self.prefetched = true;
        for (command, cache) in [
            ("list_services", "services"),
            ("list_startup", "startup"),
            ("list_connections", "connections"),
        ] {
            if !self.data.contains_key(cache) {
                self.request(command, json!({}), Some(cache), window, cx);
            }
        }
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
            "settings" => self.poll_settings(window, cx),
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
            Action::Sort(key) => {
                self.descending = if self.sort == key {
                    !self.descending
                } else {
                    tables::descending_first(&self.page, &key)
                };
                self.sort = key;
                self.sorts
                    .insert(self.page.clone(), (self.sort.clone(), self.descending));
                if self.page == "processes" {
                    self.prefs
                        .set("sort", json!({"key":self.sort,"desc":self.descending}));
                }
                self.rebuild_rows(false, cx);
            }
            Action::Page(page) => {
                if !PAGES.iter().any(|p| p.0 == page) {
                    return;
                }
                if self.page != page {
                    self.page_epoch = self.page_epoch.wrapping_add(1);
                    let index = |name: &str| PAGES.iter().position(|p| p.0 == name).unwrap_or(0);
                    self.page_direction = (index(&page) as f32 - index(&self.page) as f32).signum();
                    self.page_switched = Instant::now();
                    self.page_enter_pending = true;
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
                        tables::initial_sort(&self.page)
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
                if key.is_empty()
                    || (self.page == "processes"
                        && self.selected.get(&self.page) == Some(&key)
                        && self.last_select.as_ref().is_some_and(|(prev, at)| {
                            prev == &key && at.elapsed() > Duration::from_millis(300)
                        }))
                {
                    self.selected.remove(&self.page);
                    self.last_select = None;
                } else {
                    self.selected.insert(self.page.clone(), key.clone());
                    self.last_select = Some((key.clone(), Instant::now()));
                    if self.page == "processes" {
                        self.details_track(&key, window, cx);
                    }
                }
            }
            Action::Fold(key) => {
                if let Some(section) = key.strip_prefix("s:") {
                    // While searching every match shows: headings cannot fold.
                    if !self.query.trim().is_empty() {
                        return;
                    }
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
            Action::Interval(ms) => {
                self.interval = ms;
                self.prefs.set("interval", ms);
                self.request("set_interval", json!({"ms":ms}), None, window, cx);
            }
            Action::Device(device) => {
                self.device = device;
                self.prefs.set("perfDevice", &self.device);
                if self.page != "performance" {
                    self.act(Action::Page("performance".into()), window, cx);
                }
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
            Action::Copy(value, what) => {
                cx.write_to_clipboard(ClipboardItem::new_string(value));
                self.notify_toast_for(
                    widgets::ToastKind::Success,
                    tr!("actions.copied", what = label(what)),
                    1800,
                );
            }
            Action::CloseOverlay => self.close_overlay(window, cx),
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
        if self.page == "history"
            && self.modal.is_none()
            && self.popup.is_none()
            && self.history_key(key, m.shift, window, cx)
        {
            cx.stop_propagation();
            return;
        }
        let searching = self.search.read(cx).focus_handle(cx).is_focused(window);
        if key == "escape" {
            if self.popup.is_some() || self.modal.is_some() {
                self.act(Action::CloseOverlay, window, cx);
            } else if searching || !self.query.is_empty() {
                // Esc in the search box clears it and gives the keyboard back to the list.
                self.search.update(cx, |s, cx| s.set_value("", window, cx));
                self.query.clear();
                self.rebuild_rows(false, cx);
                self.focus.focus(window, cx);
                cx.notify();
            } else {
                self.selected.remove(&self.page);
                cx.notify();
            }
            return;
        }
        // Enter in a dialog belongs to the dialog (dialogs.rs, dialog_enter).
        if self.modal.is_some() {
            return;
        }
        if m.control && !m.alt && !m.shift && !m.platform {
            if let Ok(index) = key.parse::<usize>()
                && index > 0
                && index <= PAGES.len()
            {
                self.act(Action::Page(PAGES[index - 1].0.into()), window, cx);
                cx.stop_propagation();
                return;
            }
            if key == "f" {
                self.focus_process_search(window, cx);
                cx.stop_propagation();
                return;
            }
        }
        if searching {
            // ↓ leaves the search box for the list, selecting the first row if none is.
            if key == "down" && !self.rows.is_empty() {
                if !self
                    .selected
                    .get(&self.page)
                    .is_some_and(|key| self.rows.iter().any(|row| &row.key == key))
                {
                    let first = self.rows[0].key.clone();
                    self.act(Action::Select(first), window, cx);
                }
                self.focus.focus(window, cx);
                cx.stop_propagation();
            }
            return;
        }
        let list_page = matches!(
            self.page.as_str(),
            "processes" | "services" | "startup" | "network"
        );
        let selected = self
            .selected
            .get(&self.page)
            .and_then(|key| self.rows.iter().position(|r| &r.key == key));
        if list_page && matches!(key, "up" | "down" | "home" | "end" | "pageup" | "pagedown") {
            let last = self.rows.len().saturating_sub(1);
            // A page is the rows that fit, less one, as in the original.
            let page = self
                .table
                .as_ref()
                .map(|table| {
                    let height = table
                        .read(cx)
                        .vertical_scroll_handle
                        .0
                        .borrow()
                        .base_handle
                        .bounds()
                        .size
                        .height;
                    ((f32::from(height) / 30.).floor() as usize).saturating_sub(1)
                })
                .unwrap_or(10)
                .max(1);
            let index = match (key, selected) {
                ("up", Some(i)) => i.saturating_sub(1),
                ("down", Some(i)) => (i + 1).min(last),
                ("pageup", Some(i)) => i.saturating_sub(page),
                ("pagedown", Some(i)) => (i + page).min(last),
                ("end", _) => last,
                _ => 0,
            };
            // Category headings are not selectable: step past them in the move's direction.
            let forward = matches!(key, "down" | "pagedown" | "home");
            let index = if forward {
                (index..self.rows.len()).find(|i| self.rows[*i].section.is_none())
            } else {
                (0..=index.min(last))
                    .rev()
                    .find(|i| self.rows[*i].section.is_none())
            }
            .or_else(|| self.rows.iter().position(|row| row.section.is_none()))
            .unwrap_or(index);
            if let Some(key) = self.rows.get(index).map(|row| row.key.clone()) {
                // Keyboard moves select; they never toggle the selection off.
                self.last_select = None;
                self.act(Action::Select(key), window, cx);
                if let Some(table) = &self.table {
                    table.update(cx, |state, cx| state.scroll_to_row(index, cx));
                }
            }
            cx.stop_propagation();
            return;
        }
        if matches!(key, "left" | "right") && self.page == "processes" {
            if let Some(index) = selected {
                let row = self.rows[index].clone();
                if row.expandable && row.expanded == (key == "left") {
                    self.act(Action::Fold(row.key), window, cx);
                } else {
                    let next = if key == "right" && row.expanded {
                        self.rows.get(index + 1)
                    } else if key == "left" && row.section.is_none() {
                        // From a top-level row, ← goes to its category heading.
                        self.rows[..index].iter().rev().find(|parent| {
                            parent.depth < row.depth || (row.depth == 0 && parent.section.is_some())
                        })
                    } else {
                        None
                    };
                    if let Some(next) = next {
                        let key = next.key.clone();
                        self.last_select = None;
                        self.act(Action::Select(key), window, cx);
                    }
                }
                cx.stop_propagation();
            }
            return;
        }
        if matches!(key, "enter" | "space")
            && let Some(index) = selected
        {
            let row = self.rows[index].clone();
            if row.section.is_some() {
                self.act(Action::Fold(row.key), window, cx);
            } else if key == "enter" {
                self.activate_row(&row, window, cx);
            }
            cx.stop_propagation();
            return;
        }
        if self.page == "processes" && (key == "delete" || (m.alt && !m.shift && key == "e")) {
            self.open_selected_action("end", window, cx);
            if key == "delete" && m.shift {
                self.confirm_dialog(window, cx);
            }
            return;
        }
        if key == "/" && !m.control && !m.alt {
            self.focus_process_search(window, cx);
            cx.stop_propagation();
            return;
        }
        // Typing a letter or digit on the process list starts or extends a search.
        if self.page == "processes"
            && !m.control
            && !m.alt
            && !m.platform
            && !event.is_held
            && let Some(typed) = event.keystroke.key_char.as_deref()
            && typed.chars().count() == 1
            && typed.chars().all(char::is_alphanumeric)
        {
            let query = format!("{}{typed}", self.query);
            self.search.update(cx, |state, cx| {
                state.set_value(query.clone(), window, cx);
                state.focus(window, cx);
            });
            self.query = query;
            self.rebuild_rows(false, cx);
            cx.stop_propagation();
            cx.notify();
        }
    }
    /// Ctrl+F and "/": the process search, with its text selected.
    fn focus_process_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.page != "processes" {
            self.act(Action::Page("processes".into()), window, cx);
        }
        self.search.update(cx, |state, cx| state.focus(window, cx));
        window.defer(cx, |window, cx| {
            window.dispatch_action(Box::new(gpui_kit::component::input::SelectAll), cx);
        });
    }
    /// Enter or a double click: what the row's page does with it.
    pub(in crate::frontend) fn activate_row(
        &mut self,
        row: &Row,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.page.as_str() {
            "processes" => {
                self.details_open = true;
                self.prefs.set("detailsPanel", true);
                cx.notify();
            }
            "startup" => {
                if row.raw["blocked"].is_null() {
                    self.act(
                        Action::Invoke(
                            "set_startup_enabled".into(),
                            json!({"id": row.raw["id"], "enabled": !boolean(&row.raw, "enabled")}),
                        ),
                        window,
                        cx,
                    );
                }
            }
            _ => {
                if row.raw["pid"].as_u64().is_some_and(|pid| pid != 0) {
                    self.act(
                        Action::Preference("revealProcess".into(), json!({"pid": row.raw["pid"]})),
                        window,
                        cx,
                    );
                }
            }
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
        if self.options.fixture.is_some()
            && let Some(time) = control["chartTime"]
                .as_f64()
                .filter(|time| time.is_finite())
        {
            self.chart_clock.borrow_mut().pause_at(time);
            if let Some(layer) = self.chart_layer.clone() {
                window.on_next_frame(move |window, cx| {
                    layer.update(cx, |layer, cx| {
                        let before = layer.fast_frames;
                        layer.advance(window, cx);
                        println!(
                            "{}",
                            json!({"event":"ui-chart-step","retained":layer.fast_frames>before})
                        );
                    });
                });
            }
            return;
        }
        if self.options.fixture.is_some() && control["fullPaint"].as_bool() == Some(true) {
            window.refresh();
            return;
        }
        if control["exit"].as_bool() == Some(true) {
            self.host.exit(0);
            return;
        }
        if control["pointerOutside"].as_bool() == Some(true) {
            window.defer(cx, |window, cx| {
                window.dispatch_event(
                    PlatformInput::MouseMove(MouseMoveEvent {
                        position: point(px(-1.), px(-1.)),
                        ..Default::default()
                    }),
                    cx,
                );
            });
        }
        if let Some(wheel) = control.get("wheel") {
            // A mouse-wheel notch at a point (test only): {"x":…,"y":…,"lines":…}.
            let event = ScrollWheelEvent {
                position: point(px(number(wheel, "x") as f32), px(number(wheel, "y") as f32)),
                delta: ScrollDelta::Lines(point(0., number(wheel, "lines") as f32)),
                touch_phase: TouchPhase::Moved,
                ..Default::default()
            };
            window.defer(cx, move |window, cx| {
                if let Some(ms) = scrollbar::trace_ms() {
                    println!("{}", json!({"event":"wheel","ms":ms}));
                }
                window.dispatch_event(PlatformInput::ScrollWheel(event), cx);
            });
        }
        if let Some(ms) = control["scrollTrace"].as_u64() {
            // Print every scrollbar's offset and thumb on each frame (test only).
            scrollbar::trace(ms);
            window.refresh();
        }
        if let Some(mouse) = control.get("mouse") {
            // A left-button pointer step at a point (test only): {"type":"move|down|up","x":…,"y":…}.
            let position = point(px(number(mouse, "x") as f32), px(number(mouse, "y") as f32));
            let input = match mouse["type"].as_str() {
                Some("down") => PlatformInput::MouseDown(MouseDownEvent {
                    position,
                    button: MouseButton::Left,
                    click_count: 1,
                    ..Default::default()
                }),
                Some("up") => PlatformInput::MouseUp(MouseUpEvent {
                    position,
                    button: MouseButton::Left,
                    click_count: 1,
                    ..Default::default()
                }),
                Some("drag") => PlatformInput::MouseMove(MouseMoveEvent {
                    position,
                    pressed_button: Some(MouseButton::Left),
                    ..Default::default()
                }),
                _ => PlatformInput::MouseMove(MouseMoveEvent {
                    position,
                    ..Default::default()
                }),
            };
            window.defer(cx, move |window, cx| {
                window.dispatch_event(input, cx);
            });
        }
        if let Some(id) = control["hoverChart"].as_str()
            && let Some(position) = self.chart_center(id)
        {
            // Dispatch after releasing Desktop; hover listeners may update it.
            window.defer(cx, move |window, cx| {
                window.dispatch_event(
                    PlatformInput::MouseMove(MouseMoveEvent {
                        position,
                        ..Default::default()
                    }),
                    cx,
                );
            });
        }
        if let Some(label) = control["inspectLayout"].as_str() {
            if std::env::var_os("ARGUS_GPUI_VIEW_TRACE").is_some() {
                use gpui_kit::gpui::profiler::{self, FrameEvent, FrameTimingCollector};
                let collector = self.frame_trace.get_or_insert_with(|| {
                    profiler::set_trace_enabled(true);
                    FrameTimingCollector::new()
                });
                let mut draws = collector
                    .collect_unseen()
                    .into_iter()
                    .filter_map(|event| match event {
                        FrameEvent::Draw(frame) => Some(frame),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                draws.sort_by_key(|frame| std::cmp::Reverse(frame.draw_duration()));
                println!(
                    "{}",
                    json!({"event":"ui-draw-phases","label":label,"draws":draws.len(),"slowest":draws.iter().take(12).map(|frame|json!({"totalUs":frame.draw_duration().as_micros(),"requestUs":frame.phases.request_layout.as_micros(),"prepaintUs":frame.phases.prepaint.as_micros(),"paintUs":frame.phases.paint.as_micros(),"invalidations":frame.invalidations})).collect::<Vec<_>>()})
                );
                use profiler::journal::{ForegroundEvent, ForegroundJournalEntry};
                let journal = self
                    .view_trace
                    .get_or_insert_with(|| cx.foreground_journal().collector());
                let mut views = journal
                    .collect_unseen()
                    .entries
                    .into_iter()
                    .filter_map(|entry| match entry {
                        ForegroundJournalEntry::Event(ForegroundEvent::View(view)) => Some(view),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                views.sort_by_key(|view| std::cmp::Reverse(view.self_duration));
                println!(
                    "{}",
                    json!({"event":"ui-view-phases","label":label,"slowest":views.iter().take(16).map(|view|json!({"name":view.name,"phase":view.phase.as_str(),"selfUs":view.self_duration.as_micros(),"elapsedUs":view.end.duration_since(view.start).as_micros()})).collect::<Vec<_>>()})
                );
            }
            println!(
                "{}",
                json!({"event":"ui-conditions","label":label,"processes":self.snapshot["system"]["processCount"],"intervalMs":self.interval,"dpiScale":window.scale_factor(),"gpus":values(&self.snapshot["hardware"]["gpus"]).iter().map(|gpu|json!({"id":gpu["info"]["id"],"clockMhz":gpu["clockMhz"],"clockMaxMhz":gpu["clockMaxMhz"],"boardPowerWatts":gpu["boardPower"]})).collect::<Vec<_>>()})
            );
            let fast_frames = self
                .chart_layer
                .as_ref()
                .map_or(0, |layer| layer.read(cx).fast_frames);
            println!(
                "{}",
                json!({"event":"ui-retained-frames","label":label,"fastFrames":fast_frames})
            );
            println!(
                "{}",
                json!({"event":"ui-work","label":label,"serializeUs":self.work.serialize_us,"backgroundGroupUs":self.work.background_group_us,"timelineUs":self.work.timeline_us,"rowsUs":self.work.rows_us,"rowsMaxUs":self.work.rows_max_us,"rowBuilds":self.work.row_builds})
            );
            let (builds, micros) = charts::raster_metrics();
            println!(
                "{}",
                json!({"event":"ui-chart-work","label":label,"rasterBuilds":builds,"rasterMicros":micros})
            );
            let frames = window.frame_duration_snapshot();
            let controls = window.window_control_regions();
            let control_matches = |area| {
                controls
                    .iter()
                    .find(|(kind, _)| *kind == area)
                    .is_some_and(|(_, bounds)| {
                        window.window_control_area_at(bounds.center()) == Some(area)
                    })
            };
            let drag_points = controls
                .iter()
                .filter(|(area, _)| *area == WindowControlArea::Drag)
                .map(|(_, bounds)| {
                    (0..(f32::from(bounds.size.width) / 8.) as usize)
                        .filter(|index| {
                            window.window_control_area_at(point(
                                bounds.left() + px(*index as f32 * 8.),
                                bounds.center().y,
                            )) == Some(WindowControlArea::Drag)
                        })
                        .count()
                })
                .sum::<usize>();
            let actions_are_client = window
                .window_control_area_at(self.toolbar_regions.borrow()[1].center())
                .is_none();
            println!(
                "{}",
                json!({"event":"ui-caption","label":label,"registered":controls.len(),"dragPoints":drag_points,"actionsAreClient":actions_are_client,
                "minimize":control_matches(WindowControlArea::Min),"maximize":control_matches(WindowControlArea::Max),"close":control_matches(WindowControlArea::Close)})
            );
            println!(
                "{}",
                json!({"event":"ui-frame-duration","label":label,
                "drawP50Ms":frames.draw_duration_histogram.value_at_quantile(0.5) as f64/1e6,
                "drawP95Ms":frames.draw_duration_histogram.value_at_quantile(0.95) as f64/1e6,
                "drawP99Ms":frames.draw_duration_histogram.value_at_quantile(0.99) as f64/1e6,
                "drawMaxMs":frames.draw_duration_histogram.max() as f64/1e6})
            );
            println!(
                "{}",
                json!({"event":"ui-layout","label":label,"page":self.page,"device":self.device,
                "width":f32::from(window.viewport_size().width),"height":f32::from(window.viewport_size().height),
                "performanceScrollY":f32::from(self.device_scroll.max_offset().y),
                "settingsScrollY":f32::from(self.settings_scroll.max_offset().y),"settingsTop":self.settings_scroll.top_item()})
            );
        }
        if self.options.fixture.is_some()
            && let Some(key) = control["clickHeader"].as_str()
            && let Some(bounds) = self.header_bounds.borrow().get(key).copied()
        {
            // Exercise the real hit target, away from the text and sort indicator.
            let position = point(
                bounds.left() + bounds.size.width * 0.15,
                bounds.top() + bounds.size.height * 0.25,
            );
            window.defer(cx, move |window, cx| {
                window.dispatch_event(
                    PlatformInput::MouseMove(MouseMoveEvent {
                        position,
                        ..Default::default()
                    }),
                    cx,
                );
                window.dispatch_event(
                    PlatformInput::MouseDown(MouseDownEvent {
                        position,
                        button: MouseButton::Left,
                        click_count: 1,
                        ..Default::default()
                    }),
                    cx,
                );
                window.dispatch_event(
                    PlatformInput::MouseUp(MouseUpEvent {
                        position,
                        button: MouseButton::Left,
                        click_count: 1,
                        ..Default::default()
                    }),
                    cx,
                );
                window.dispatch_event(
                    PlatformInput::MouseMove(MouseMoveEvent {
                        position: point(px(-1.), px(-1.)),
                        ..Default::default()
                    }),
                    cx,
                );
            });
        }
        if self.options.fixture.is_some()
            && let Some(data) = control["data"].as_object()
        {
            for (key, value) in data {
                self.data.insert(key.clone(), value.clone());
            }
        }
        if let Some(text) = control["toast"].as_str() {
            self.notify_toast(widgets::ToastKind::Info, text.to_owned());
        }
        if control["recordHotkey"].as_bool() == Some(true) {
            self.begin_hotkey(window, cx);
        }
        if self.options.fixture.is_some()
            && let Some(keys) = control["keys"].as_array()
        {
            // Synthetic keys through the normal dispatch path, for dialogs and the recorder.
            let strokes = keys
                .iter()
                .filter_map(Value::as_str)
                .filter_map(|k| Keystroke::parse(k).ok())
                .collect::<Vec<_>>();
            window.defer(cx, move |window, cx| {
                for stroke in strokes {
                    let modifiers = |modifiers| {
                        PlatformInput::ModifiersChanged(ModifiersChangedEvent {
                            modifiers,
                            capslock: Default::default(),
                        })
                    };
                    window.dispatch_event(modifiers(stroke.modifiers), cx);
                    window.dispatch_event(
                        PlatformInput::KeyDown(KeyDownEvent {
                            keystroke: stroke.clone(),
                            is_held: false,
                            prefer_character_input: false,
                        }),
                        cx,
                    );
                    window
                        .dispatch_event(PlatformInput::KeyUp(KeyUpEvent { keystroke: stroke }), cx);
                    window.dispatch_event(modifiers(Modifiers::default()), cx);
                }
            });
        }
        if self.options.fixture.is_some()
            && let Some(id) = control["clickSelect"].as_str()
            && let Some(bounds) = self.ui.anchors.borrow().get(id).copied()
        {
            let position = bounds.center();
            window.defer(cx, move |window, cx| {
                for event in [
                    PlatformInput::MouseMove(MouseMoveEvent {
                        position,
                        ..Default::default()
                    }),
                    PlatformInput::MouseDown(MouseDownEvent {
                        position,
                        button: MouseButton::Left,
                        click_count: 1,
                        ..Default::default()
                    }),
                    PlatformInput::MouseUp(MouseUpEvent {
                        position,
                        button: MouseButton::Left,
                        click_count: 1,
                        ..Default::default()
                    }),
                ] {
                    window.dispatch_event(event, cx);
                }
            });
        }
        if let Some(section) = control["settingsSection"].as_str() {
            self.preference("settingsSection", json!(section), window, cx);
        }
        if let Some(label) = control["inspectLayout"].as_str() {
            println!(
                "{}",
                json!({"event":"ui-render-counts","label":label,"desktop":self.render_count,"chartTicks":self.chart_frame,"sort":self.sort,"descending":self.descending})
            );
        }
        // The page's rows as built, to compare titles and order with the original.
        if let Some(label) = control["dumpRows"].as_str() {
            let rows: Vec<Value> = self
                .rows
                .iter()
                .map(|row| {
                    let count = if row.section.is_some() {
                        row.raw["entryCount"].clone()
                    } else {
                        json!(row.members.len())
                    };
                    json!({"key":row.key,"title":row.title,"detail":row.detail,"depth":row.depth,"count":count,"section":row.section,"gpu":row.metric("gpu")})
                })
                .collect();
            println!(
                "{}",
                json!({"event":"rows","label":label,"page":self.page,"view":self.view,"sort":self.sort,"descending":self.descending,"rows":rows})
            );
        }
        if let Some(key) = control["sort"].as_str() {
            self.act(Action::Sort(key.into()), window, cx);
        }
        if let Some(index) = control["foldRow"].as_u64()
            && let Some(row) = self.rows.get(index as usize)
        {
            self.act(Action::Fold(row.key.clone()), window, cx);
        }
        for key in ["navPosition", "navExpanded", "coreFilter", "uiMotion"] {
            if let Some(value) = control.get(key) {
                self.preference(key, value.clone(), window, cx);
            }
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
        if let Some(index) = control["rowMenu"].as_u64()
            && let Some(row) = self.rows.get(index as usize).cloned()
        {
            self.selected.insert(self.page.clone(), row.key.clone());
            let items = self.row_menu(&row);
            self.open_popup(items, window, cx);
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
