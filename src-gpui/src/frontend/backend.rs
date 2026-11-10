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
                | "set_hotkey"
                | "set_hotkey_enabled"
                | "suspend_hotkey"
                | "resume_hotkey"
                | "quit"
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
        // An answer for another range or moment is dropped. One history query of each
        // kind is in flight at a time, so the one for what is shown now may have been
        // held back while this one was read: forget it was sent and ask again.
        if pending.command == "history_series" && pending.args["range"] != self.history_range {
            self.data.remove("historyQuery");
            self.load_history(window, cx);
            return;
        }
        if pending.command == "history_moment"
            && pending.args["cursor"] != json!(self.history_cursor)
        {
            self.data.remove("historyMomentAt");
            self.load_history(window, cx);
            return;
        }
        match result {
            Ok(value) if pending.command == "list_connections" => {
                // The rows are built off the UI thread from the shared list
                // and installed when ready; an identical read changes nothing.
                if self.set_connections(value) {
                    self.refresh_connections(cx);
                }
                return;
            }
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
                if pending.command == "history_series" {
                    self.history_answered(&pending.args, window, cx);
                }
                if pending.command.starts_with("set_") && value.get("closeToTray").is_some() {
                    self.data.insert("settings".into(), value.clone());
                }
                self.command_completed(&pending.command, &value, window, cx);
                if self.page == "network" {
                    // Its rows follow the list, its owners and how the page
                    // shows them: rebuilt only when one of those changed.
                    self.refresh_connections(cx);
                } else {
                    self.rebuild_rows(
                        self.pointer_in_table || self.modal.is_some() || self.popup.is_some(),
                        cx,
                    );
                }
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
        for mut event in self.host.drain() {
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
                    // Taken, not copied: a list read can hold thousands of entries.
                    if event.value["ok"].as_bool().unwrap_or(false) {
                        Ok(event.value["value"].take())
                    } else {
                        Err(event.value["value"].take())
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
                    changed = true;
                }
                "argus-blur" => {
                    self.set_control_paused(false, window, cx);
                    self.hotkey_blur(window, cx);
                }
                "argus-restored" => self.restored(&event.value, window, cx),
                "argus-hotkey-changed" => self.hotkey_changed(window, cx),
                "argus-show-update" => self.open_dialog("update", json!({}), window, cx),
                "argus-files-dropped" => {
                    if let Ok(paths) = serde_json::from_value::<Vec<String>>(event.value) {
                        changed = true;
                        self.accept_file_paths(paths, window, cx);
                    }
                }
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
                    self.tree_expanded.clone(),
                    self.sections.clone(),
                    self.hosted_services(),
                    self.visibility_rules("processes"),
                    self.user_grouping(),
                    self.user_folds.clone(),
                );
                let task = cx.background_executor().spawn(async move {
                    let start = Instant::now();
                    let model = serde_json::to_value(snapshot.as_ref()).expect("snapshot model");
                    let serialize_us = start.elapsed().as_micros() as u64;
                    let start = Instant::now();
                    let rows = processes.then(|| {
                        let users =
                            model::UserGroups::grouping(values(&model["sessions"]), options.9);
                        model::process_rows(
                            values(&model["processes"]),
                            model::ProcessOptions {
                                mode: &options.0,
                                query: &options.1,
                                sort: &options.2,
                                descending: options.3,
                                expanded: &options.4,
                                tree_expanded: &options.5,
                                sections: &options.6,
                                hosted: &options.7,
                                visibility: options.8.as_ref(),
                                users: users.as_ref(),
                                user_folds: &options.10,
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
                        let full = this.snapshot.is_null()
                            || this.snapshot["hardware"]["cpuSensors"]["state"]
                                != model["hardware"]["cpuSensors"]["state"];
                        this.accept_snapshot_prepared(model, rows, window, cx);
                        super::trace_work("snapshot", None, started);
                        // Pick up the newest source sequence without accumulating a UI queue.
                        this.notifications(window, cx);
                        if this.list_page_entering(cx) {
                            // Only the status bar shows the sample there.
                            this.after_entrance(cx);
                        } else {
                            this.notify_snapshot(full, window, cx);
                        }
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
    pub(in crate::frontend) fn accept_snapshot(
        &mut self,
        snapshot: Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.accept_snapshot_prepared(snapshot, None, window, cx);
    }

    /// List data arrives through its own command/row notifications. A sample
    /// only changes the uncached status bar on these pages. Settings also read
    /// sensor state and live process icons for rules, so retain full updates there.
    pub(in crate::frontend) fn notify_snapshot(
        &self,
        full: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let status_only = matches!(
            self.page.as_str(),
            "services" | "startup" | "network" | "about"
        ) || (self.page == "settings"
            && values(&self.data["settings"]["rules"]).is_empty());
        let visible = self.host.get_window("main").is_some_and(|window| {
            window.is_visible().unwrap_or(false) && !window.is_minimized().unwrap_or(false)
        });
        if status_only
            && !full
            && visible
            && self.modal.is_none()
            && self.popup.is_none()
            && self.drag.is_none()
        {
            window.request_view_redraw(cx.entity_id());
        } else {
            cx.notify();
        }
    }
    fn accept_snapshot_prepared(
        &mut self,
        snapshot: Value,
        rows: Option<Vec<Row>>,
        window: &mut Window,
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
        let icon_ids = values(&snapshot["processes"])
            .iter()
            .flat_map(|process| {
                [
                    process["icon"].as_u64(),
                    process["webView"]["hostIcon"].as_u64(),
                ]
                .into_iter()
                .flatten()
                .filter_map(|id| u32::try_from(id).ok())
            })
            .collect();
        self.ui
            .icons
            .borrow_mut()
            .synchronize(icon_ids, |id| self.icon_png(id), window, cx);
        let start = Instant::now();
        self.timeline.push(&snapshot, self.interval as f64);
        self.sensor_layout.observe(&snapshot["hardware"]);
        self.work.timeline_us += start.elapsed().as_micros() as u64;
        let previous = std::mem::replace(&mut self.snapshot, snapshot);
        // New processes are tinted for a few samples.
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
        self.tree_expanded
            .retain(|key| self.first_seen.contains_key(key));
        if !previous.is_null() {
            cx.background_executor()
                .spawn(async move { drop(previous) })
                .detach();
        }
        if self.page == "processes" {
            self.rebuild_rows_prepared(
                self.pointer_in_table || self.popup.is_some() || self.modal.is_some(),
                rows,
                cx,
            );
        } else if self.page == "network" {
            // Connections take their owners' names and icons from the sample:
            // rebuilt off the UI thread, and only when those changed.
            self.refresh_connections(cx);
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
    /// How often the visible page re-reads its list.
    pub(in crate::frontend) fn poll_interval(&self) -> Duration {
        Duration::from_millis(match self.page.as_str() {
            "services" | "processes" => 3000,
            "startup" => 10000,
            _ => 2000,
        })
    }
    /// Once the window has been hidden or minimized for the chosen minutes
    /// (`memory.releaseHidden`, 0 for never), drops the chart textures and
    /// gives the process memory back. Showing the window rasters the charts
    /// again before its first frame.
    pub(in crate::frontend) fn release_hidden_memory(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (since, released) = *self.hidden_since.get_or_insert((Instant::now(), false));
        let minutes = self.release_hidden_minutes();
        if released || minutes == 0 || since.elapsed() < Duration::from_secs(minutes * 60) {
            return;
        }
        self.hidden_since = Some((since, true));
        charts::release_textures(window, cx);
        crate::memory_pressure::release_process_memory();
    }
    /// The chosen `memory.releaseHidden`; a hand edit to a value on offer
    /// nowhere reads as the default.
    pub(in crate::frontend) fn release_hidden_minutes(&self) -> u64 {
        let minutes = self
            .prefs
            .get::<u64>("memory.releaseHidden", DEFAULT_RELEASE_HIDDEN_MINUTES);
        if RELEASE_HIDDEN_MINUTES.contains(&minutes) {
            minutes
        } else {
            DEFAULT_RELEASE_HIDDEN_MINUTES
        }
    }
    /// Reads the lists of the other table pages once, a second after the first
    /// snapshot, so that the first visit shows them at once instead of waiting
    /// for the read (cold, the service list alone took over a second). Each page
    /// still re-reads its list when shown; the process page already reads the
    /// services. The connection rows are built from the read off the UI thread
    /// (`refresh_connections`), so that the first visit does not build them.
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
            let read = if cache == "connections" {
                self.connections.is_some()
            } else {
                self.data.contains_key(cache)
            };
            if !read {
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
            "about" => self.poll_about(window, cx),
            "history" => self.load_history(window, cx),
            _ => {}
        }
    }
    /// The locale key of the search box placeholder on the current page.
    pub(in crate::frontend) fn search_placeholder(&self) -> &'static str {
        match self.page.as_str() {
            "services" => "services.search",
            "startup" => "startup.search",
            "network" => "connections.search",
            _ => "shell.search.processes",
        }
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
        let key = if command == "set_suspended" && kind == "critical" {
            // The general text is about ending the process.
            "actions.suspend.critical".to_owned()
        } else {
            format!(
                "{}.{kind}",
                if command == "run_task" {
                    "tasks.error"
                } else {
                    "actions.error"
                }
            )
        };
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
                if !tables::sortable(&self.page, &key) {
                    return;
                }
                self.descending = if self.sort == key {
                    !self.descending
                } else {
                    tables::descending_first(&self.page, &key)
                };
                self.sort = key;
                self.sorts
                    .insert(self.page.clone(), (self.sort.clone(), self.descending));
                if let Some((key, value)) =
                    tables::sort_preference(&self.page, &self.sort, self.descending)
                {
                    self.prefs.set(&key, value);
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
                        tables::saved_sort(&self.page, &self.prefs)
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
                self.horizontal = 0.;
                self.popup = None;
                // The network page shows its last rows when they were built for
                // how it shows them now, without building thousands of rows
                // while it enters; newer ones replace them after the entrance.
                if !(self.page == "network" && self.show_kept_connections(cx)) {
                    self.rebuild_rows(false, cx);
                }
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
                if let Some(group) = key.strip_prefix("u:") {
                    // While searching every match shows: headings cannot fold.
                    if !self.query.trim().is_empty() {
                        return;
                    }
                    self.fold_user(group, !self.user_folds.contains(group));
                } else if let Some(section) = key.strip_prefix("s:") {
                    if !self.query.trim().is_empty() {
                        return;
                    }
                    // A category folds in every user's group at once.
                    let section = model::split_scope(section).0;
                    if !self.sections.remove(section) {
                        self.sections.insert(section.into());
                    }
                    self.prefs.set(
                        "collapsedSections",
                        self.sections.iter().collect::<Vec<_>>(),
                    );
                } else if self.view == "tree" {
                    if let Some(row) = self
                        .rows
                        .iter()
                        .find(|row| row.key == key && row.expandable)
                    {
                        self.act(Action::TreeExpand(key, !row.expanded), window, cx);
                    }
                    return;
                } else {
                    let set = &mut self.expanded;
                    if !set.remove(&key) {
                        set.insert(key);
                    }
                }
                self.rebuild_rows(false, cx);
            }
            Action::TreeExpand(key, expand) => {
                if self.page != "processes"
                    || self.view != "tree"
                    || !self.query.trim().is_empty()
                    || !self.rows.iter().any(|row| row.key == key && row.expandable)
                {
                    return;
                }
                if expand {
                    self.tree_expanded.insert(key);
                } else {
                    self.tree_expanded.remove(&key);
                }
                self.rebuild_rows(false, cx);
            }
            Action::TreeExpandAll(expand) => {
                if self.page != "processes" || self.view != "tree" || !self.query.trim().is_empty()
                {
                    return;
                }
                self.tree_expanded = if expand {
                    model::tree_parents(values(&self.snapshot["processes"]))
                } else {
                    HashSet::new()
                };
                self.rebuild_rows(false, cx);
            }
            Action::Details(mode) => {
                self.set_details_mode(mode.unwrap_or(self.details_mode.next()));
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
                // A device opened while hidden from the list returns to it.
                self.reveal_device(&device);
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
        if self.popup.is_some() {
            self.menu_key(key, window, cx);
            cx.stop_propagation();
            return;
        }
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
            // A page is the rows that fit, less one.
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
                if row.expandable && row.expanded == (key == "left") && self.query.trim().is_empty()
                {
                    self.act(Action::Fold(row.key), window, cx);
                } else {
                    let next = if key == "right" && row.expanded {
                        self.rows.get(index + 1)
                    } else if key == "left" && row.section.is_none() {
                        // From a top-level row, ← goes to its category heading.
                        self.rows[..index].iter().rev().find(|parent| {
                            parent.depth < row.depth || (row.depth == 0 && parent.section.is_some())
                        })
                    } else if key == "left" && row.nested {
                        // From a category heading, to its user's.
                        self.rows[..index]
                            .iter()
                            .rev()
                            .find(|parent| parent.user.is_some())
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
                self.reveal_details();
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
}
