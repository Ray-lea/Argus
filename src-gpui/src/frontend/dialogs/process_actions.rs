//! Process actions from dialogs and menus: ending processes and trees,
//! priorities and efficiency mode in batches, rules made from a process,
//! restarting Explorer, and the run-a-new-task dialog.
use super::*;

impl Desktop {
    /// The number of processes started under `root`, as the process tree counts them.
    pub(super) fn descendants(&self, root: &Value) -> usize {
        let processes = values(&self.snapshot["processes"]);
        let mut children: HashMap<u64, Vec<&Value>> = HashMap::new();
        for p in processes {
            children
                .entry(p["ppid"].as_u64().unwrap_or(0))
                .or_default()
                .push(p);
        }
        let mut seen = HashSet::from([root["pid"].as_u64().unwrap_or(0)]);
        let mut stack = vec![root];
        let mut count = 0;
        while let Some(p) = stack.pop() {
            for c in children
                .get(&p["pid"].as_u64().unwrap_or(0))
                .into_iter()
                .flatten()
            {
                let pid = c["pid"].as_u64().unwrap_or(0);
                if model::parent_predates(p, c) && seen.insert(pid) {
                    count += 1;
                    stack.push(c);
                }
            }
        }
        count
    }

    /// Runs one process action on each member off the UI thread and reports
    /// the outcome once, after every member has finished.
    pub(super) fn process_batch(
        &mut self,
        kind: &str,
        args: &Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let members = values(&args["members"]).to_vec();
        let name = text(args, "name").to_owned();
        if members
            .iter()
            .any(|member| number(member, "createTime") < 0.)
        {
            self.notify_toast(ToastKind::Info, label("processes.identityPending"));
            return;
        }
        if members.is_empty() {
            self.notify_toast(ToastKind::Error, label("actions.staleProcess"));
            return;
        }
        let (command, extra) = match kind {
            "end" => ("kill_process", json!({"force":false})),
            "endTree" => ("kill_process_tree", json!({})),
            "priority" => ("set_priority", json!({"priority":args["priority"]})),
            "ioPriority" => ("set_io_priority", json!({"priority":args["priority"]})),
            "memoryPriority" => ("set_memory_priority", json!({"priority":args["priority"]})),
            "suspend" => ("set_suspended", json!({"suspend":args["on"]})),
            _ => ("set_efficiency_mode", json!({"on":args["on"]})),
        };
        let payloads = if kind == "endTree" {
            members.iter().take(1).cloned().collect::<Vec<_>>()
        } else {
            members
        }
        .into_iter()
        .map(|member| {
            let mut payload = json!({"pid":member["pid"],"createTime":member["createTime"]});
            for (key, value) in extra.as_object().into_iter().flatten() {
                payload[key] = value.clone();
            }
            payload
        })
        .collect::<Vec<_>>();
        if self.options.fixture.is_some() {
            return;
        }
        self.next_request += 1;
        let id = self.next_request;
        let summary = json!({"kind":kind,"command":command,"name":name,"priority":args["priority"],"on":args["on"]});
        self.pending.insert(
            id,
            Pending {
                command: "batch".into(),
                cache: None,
                args: summary.clone(),
            },
        );
        let host = self.host.clone();
        let command = command.to_owned();
        std::thread::spawn(move || {
            let results = payloads
                .into_iter()
                .map(|payload| {
                    let result = crate::dispatch::invoke(&host, &command, payload.clone());
                    json!({"payload":payload,"ok":result.is_ok(),"value":result.unwrap_or_else(|e| e)})
                })
                .collect::<Vec<_>>();
            let mut value = summary;
            value["results"] = json!(results);
            let _ = host.emit("command-response", json!({"id":id,"ok":true,"value":value}));
        });
        let _ = window;
        cx.notify();
    }

    pub(super) fn batch_done(&mut self, value: &Value) {
        let kind = text(value, "kind");
        let command = text(value, "command");
        let name = text(value, "name").to_owned();
        let results = values(&value["results"]);
        let failed = results
            .iter()
            .filter(|r| !boolean(r, "ok"))
            .collect::<Vec<_>>();
        let total = results.len();
        let first_error = failed
            .first()
            .map(|r| self.action_error(command, &r["payload"], &r["value"]))
            .unwrap_or_default();
        let outdated = failed
            .iter()
            .any(|r| text(&r["value"], "kind") == "serviceOutdated");
        let (kind_out, text_out, ms) = match kind {
            "end" if total > 1 => {
                if failed.is_empty() {
                    (
                        ToastKind::Success,
                        tr!("actions.endGroup.done", name = name, count = total),
                        5000,
                    )
                } else {
                    (
                        ToastKind::Error,
                        tr!(
                            "actions.endGroup.partial",
                            error = first_error,
                            count = failed.len()
                        ),
                        5000,
                    )
                }
            }
            "end" => {
                if failed.is_empty() {
                    (
                        ToastKind::Success,
                        tr!("actions.end.done", name = name),
                        2500,
                    )
                } else {
                    (ToastKind::Error, first_error, 5000)
                }
            }
            "endTree" => {
                let report = &results
                    .first()
                    .map(|r| r["value"].clone())
                    .unwrap_or_default();
                if !failed.is_empty() {
                    (ToastKind::Error, first_error, 5000)
                } else if number(report, "failed") > 0. {
                    (
                        ToastKind::Error,
                        tr!(
                            "actions.endTree.partial",
                            count = number(report, "terminated") as u64,
                            failed = number(report, "failed") as u64
                        ),
                        5000,
                    )
                } else {
                    (
                        ToastKind::Success,
                        tr!(
                            "actions.endTree.done",
                            count = number(report, "terminated") as u64
                        ),
                        2500,
                    )
                }
            }
            "priority" => {
                let wanted = text(value, "priority");
                if failed.len() == total {
                    (
                        ToastKind::Error,
                        format!(
                            "{}：{first_error}",
                            tr!("actions.priority.failed", name = name)
                        ),
                        5000,
                    )
                } else if results.iter().any(|r| {
                    boolean(r, "ok") && r["value"].as_str().is_some_and(|got| got != wanted)
                }) {
                    (ToastKind::Error, label("actions.priority.downgraded"), 5000)
                } else if !failed.is_empty() {
                    (
                        ToastKind::Error,
                        tr!(
                            "actions.priority.partial",
                            count = total - failed.len(),
                            failed = failed.len()
                        ),
                        5000,
                    )
                } else {
                    (
                        ToastKind::Success,
                        tr!(
                            "actions.priority.done",
                            name = name,
                            priority = label(&format!("processes.priority.{wanted}"))
                        ),
                        2500,
                    )
                }
            }
            "ioPriority" | "memoryPriority" => {
                let wanted = text(value, "priority");
                if failed.len() == total {
                    // The whole sentence, cause included, comes from the
                    // locale, its punctuation too.
                    (
                        ToastKind::Error,
                        argus_core::i18n::format(
                            &format!("actions.{kind}.failed"),
                            &[("name", &name), ("error", &first_error)],
                        ),
                        5000,
                    )
                } else if !failed.is_empty() {
                    (
                        ToastKind::Error,
                        argus_core::i18n::format(
                            &format!("actions.{kind}.partial"),
                            &[
                                ("count", &(total - failed.len())),
                                ("failed", &failed.len()),
                                ("error", &first_error),
                            ],
                        ),
                        5000,
                    )
                } else {
                    (
                        ToastKind::Success,
                        argus_core::i18n::format(
                            &format!("actions.{kind}.done"),
                            &[
                                ("name", &name),
                                ("priority", &label(&format!("processes.{kind}.{wanted}"))),
                            ],
                        ),
                        2500,
                    )
                }
            }
            _ => {
                // Efficiency mode and suspension: on or off, with the same three outcomes.
                let action = if kind == "suspend" {
                    "suspend"
                } else {
                    "efficiency"
                };
                let state = if boolean(value, "on") { "on" } else { "off" };
                if failed.len() == total {
                    (
                        ToastKind::Error,
                        format!(
                            "{}：{first_error}",
                            argus_core::i18n::format(
                                &format!("actions.{action}.{state}.failed"),
                                &[("name", &name)]
                            )
                        ),
                        5000,
                    )
                } else if !failed.is_empty() {
                    (
                        ToastKind::Error,
                        argus_core::i18n::format(
                            &format!("actions.{action}.{state}.partial"),
                            // Suspension texts name the first cause; efficiency's ignore it.
                            &[
                                ("count", &(total - failed.len())),
                                ("failed", &failed.len()),
                                ("error", &first_error),
                            ],
                        ),
                        5000,
                    )
                } else {
                    (
                        ToastKind::Success,
                        argus_core::i18n::format(
                            &format!("actions.{action}.{state}.done"),
                            &[("name", &name)],
                        ),
                        2500,
                    )
                }
            }
        };
        if kind_out == ToastKind::Error && outdated {
            // Denied while the service needs an update: the notice leads to its card.
            self.toast_with(
                ToastKind::Error,
                text_out,
                10000,
                vec![(
                    label("settings.privilege.update"),
                    Rc::new(
                        |this: &mut Desktop, window: &mut Window, cx: &mut Context<Desktop>| {
                            this.locate_setting("privilege", "privilegedService", window, cx)
                        },
                    ),
                )],
            );
        } else {
            self.toast_with(kind_out, text_out, ms, vec![]);
        }
    }

    /// Opens Settings on the rule for `name`, creating one first.
    pub(super) fn add_rule_for(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        if name.is_empty() {
            return;
        }
        let rules = values(&self.data["settings"]["rules"]).to_vec();
        if let Some(rule) = rules
            .iter()
            .find(|r| model::same_executable_name(text(r, "name"), name))
        {
            self.ui.focus_rule = Some((text(rule, "id").to_owned(), Instant::now()));
            self.preference("settingsSection", json!("rules"), window, cx);
            return;
        }
        let processes = values(&self.snapshot["processes"]);
        let same = processes
            .iter()
            .filter(|p| model::same_executable_name(text(p, "name"), name))
            .collect::<Vec<_>>();
        let process = same.first().cloned().cloned().unwrap_or(Value::Null);
        let broad = SHARED_HOSTS.contains(&name.to_ascii_lowercase().as_str())
            || same
                .iter()
                .map(|p| text(p, "path").to_lowercase())
                .collect::<HashSet<_>>()
                .len()
                > 1;
        let display = name_of(&same).unwrap_or_else(|| name.to_owned());
        let name = name.to_owned();
        let create = move |this: &mut Desktop, window: &mut Window, cx: &mut Context<Desktop>| {
            let priority = process["priorityClass"]
                .as_str()
                .filter(|p| *p != "normal" && PRIORITIES.contains(p))
                .map(|p| json!(p))
                .unwrap_or(Value::Null);
            let id = format!(
                "native-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
            );
            let mut rules = values(&this.data["settings"]["rules"]).to_vec();
            rules.push(
                json!({"id":id,"name":name,"enabled":true,"priority":priority,
                "efficiency":if boolean(&process,"efficiency") {json!(true)} else {Value::Null}}),
            );
            this.request(
                "set_rules",
                json!({"rules":rules}),
                Some("settings"),
                window,
                cx,
            );
            this.ui.focus_rule = Some((id, Instant::now()));
            this.preference("settingsSection", json!("rules"), window, cx);
        };
        if broad {
            self.ask(
                label("actions.ruleScope.title"),
                tr!("actions.ruleScope.body", name = display),
                label("actions.ruleScope.confirm"),
                false,
                create,
                window,
                cx,
            );
        } else {
            create(self, window, cx);
        }
    }

    /// Restarts Explorer with a pending notice, then says how it went.
    pub(super) fn restart_explorer(
        &mut self,
        shell: Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.ui.busy.contains("explorer") {
            return;
        }
        self.ui.busy.insert("explorer".into());
        if boolean(&shell, "locker") {
            self.ui
                .errors
                .insert("explorerLocker".into(), String::new());
        }
        let pending = self.toast_with(
            ToastKind::Info,
            label("tasks.explorer.restarting"),
            60000,
            vec![],
        );
        self.ui
            .errors
            .insert("explorerToast".into(), pending.to_string());
        // A fixture shows the restart under way but never asks for it; a
        // test control (`resolve`) answers in its place.
        if self.options.fixture.is_some() {
            return;
        }
        self.request(
            "restart_explorer",
            json!({"pid":shell["pid"],"createTime":shell["createTime"]}),
            None,
            window,
            cx,
        );
    }

    pub(super) fn explorer_finished(&mut self, error: Option<&Value>) {
        self.ui.busy.remove("explorer");
        self.ui.errors.remove("explorerLocker");
        if let Some(id) = self
            .ui
            .errors
            .remove("explorerToast")
            .and_then(|id| id.parse::<u64>().ok())
        {
            self.dismiss_toast(id);
        }
        let Some(error) = error else {
            self.notify_toast(ToastKind::Success, label("tasks.explorer.done"));
            return;
        };
        let kind = text(error, "kind");
        let key = if matches!(
            kind,
            "stale" | "unsupported" | "permission" | "prepare" | "shutdown" | "restore" | "busy"
        ) {
            format!("tasks.explorer.error.{kind}")
        } else {
            "tasks.explorer.error.prepare".into()
        };
        let mut message = label(&key);
        if let Some(code) = hex_code(&error["code"]) {
            message = format!("{message} {code}");
        }
        let actions: Vec<(String, Handler)> = if boolean(error, "shellMissing") {
            vec![(
                label("tasks.explorer.recover"),
                Rc::new(
                    |this: &mut Desktop, window: &mut Window, cx: &mut Context<Desktop>| {
                        this.open_dialog(
                            "task",
                            json!({"command":"%WINDIR%\\explorer.exe"}),
                            window,
                            cx,
                        )
                    },
                ),
            )]
        } else {
            vec![]
        };
        self.toast_with(ToastKind::Error, message, 15000, actions);
    }

    /// Runs the new task's command.
    pub(super) fn run_task(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(modal) = self.modal.as_mut().filter(|m| m.kind == "task") else {
            return;
        };
        if self.ui.busy.contains("task") || self.ui.busy.contains("pick") {
            return;
        }
        let command = self
            .forms
            .get("value")
            .map(|v| v.read(cx).value().to_string())
            .unwrap_or_default();
        if !valid_task(&command) {
            modal.args["failure"] = json!({"kind":"invalid"});
            window.refresh();
            return;
        }
        modal.args["failure"] = Value::Null;
        let elevated = boolean(&modal.args, "elevated");
        self.ui.busy.insert("task".into());
        self.request(
            "run_task",
            json!({"command":command.trim(),"elevated":elevated}),
            None,
            window,
            cx,
        );
        window.refresh();
        cx.notify();
    }

    pub(super) fn task_dialog(
        &self,
        dialog: Dialog,
        modal: &Modal,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Dialog {
        let p = self.palette;
        let id = modal.id;
        let busy = self.ui.busy.contains("task");
        let pending = busy || self.ui.busy.contains("pick");
        let elevated = boolean(&modal.args, "elevated");
        let command = self
            .forms
            .get("value")
            .map(|v| v.read(cx).value().to_string())
            .unwrap_or_default();
        let failure = modal.args["failure"].clone();
        let failure_kind = text(&failure, "kind").to_owned();
        let shown_error = !failure_kind.is_empty() && failure_kind != "cancelled";
        let feedback = if !failure_kind.is_empty() {
            let key = format!("tasks.error.{failure_kind}");
            let message = label(&key);
            Some((
                if message == key {
                    label("tasks.error.os")
                } else {
                    message
                },
                if failure_kind == "cancelled" {
                    None
                } else {
                    hex_code(&failure["code"])
                },
            ))
        } else {
            None
        };
        // The dialog keeps its scrollbar's gutter (`scrollbar-gutter: stable`):
        // header, body and footer all end that far from its right edge.
        dialog
            .child(
                self.dialog_body(id).mr(px(scrollbar::SIZE)).child(
                    div()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .flex()
                                .items_start()
                                .gap(px(14.))
                                .pt(px(26.))
                                .px(px(24.))
                                .child(
                                    div()
                                        .size(px(44.))
                                        .flex_shrink_0()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .border_1()
                                        .border_color(p.stroke)
                                        .rounded(px(10.))
                                        .bg(p.layer_strong)
                                        .child(icon("RunTaskIcon", p.accent).size(px(24.))),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .flex()
                                        .flex_col()
                                        .gap(px(5.))
                                        .child(self.dialog_title(modal.title.clone()))
                                        .child(
                                            div()
                                                .text_size(theme::font(12.))
                                                .line_height(relative(1.6))
                                                .text_color(p.muted)
                                                .child(modal.body.clone()),
                                        ),
                                )
                                .child(
                                    self.btn(
                                        format!("modal-{id}-x"),
                                        "",
                                        Some("DismissIcon"),
                                        Tone::Plain,
                                    )
                                    .mt(px(-8.))
                                    .mr(px(-8.))
                                    .accessibility_label(label("common.close"))
                                    .inert(pending)
                                    .on_click(cx.listener(
                                        |this, _, window, cx| this.close_overlay(window, cx),
                                    )),
                                ),
                        )
                        .child(
                            div()
                                .pt(px(22.))
                                .px(px(24.))
                                .pb(px(8.))
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .mb(px(8.))
                                        .font_weight(theme::LABEL_WEIGHT)
                                        .child(label("tasks.command")),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .gap(px(8.))
                                        .child(div().flex_1().min_w_0().h(px(36.)).when_some(
                                            self.forms.get("value"),
                                            |d, input| {
                                                // Commands in the monospaced face, the
                                                // placeholder in the interface one. While
                                                // a run is pending, disable
                                                // the box without changing how it looks.
                                                d.child(
                                                    self.text_field(
                                                        input,
                                                        widgets::input(input)
                                                            .font_family(theme::MONO_FONT)
                                                            .readonly(pending),
                                                        Field::Bare,
                                                        label("tasks.placeholder").into(),
                                                        !pending,
                                                        window,
                                                        cx,
                                                    ),
                                                )
                                            },
                                        ))
                                        .child(
                                            self.btn(
                                                format!("modal-{id}-browse"),
                                                label("tasks.browse"),
                                                Some("FolderIcon"),
                                                Tone::Plain,
                                            )
                                            // A 32 px `.btn` at the top of the 36 px row.
                                            .flex_shrink_0()
                                            .inert(pending)
                                            .on_click(
                                                cx.listener(|this, _, window, cx| {
                                                    if this.options.fixture.is_some() {
                                                        return;
                                                    }
                                                    this.ui.busy.insert("pick".into());
                                                    this.request(
                                                        "pick_path",
                                                        json!({"folder":false,"task":true}),
                                                        None,
                                                        window,
                                                        cx,
                                                    );
                                                }),
                                            ),
                                        ),
                                )
                                .child(
                                    div()
                                        .id(("task-elevated", id))
                                        .mt(px(20.))
                                        .flex()
                                        .items_start()
                                        .gap(px(10.))
                                        .cursor_pointer()
                                        .when(!pending, |d| {
                                            d.on_click(cx.listener(move |this, _, window, cx| {
                                                if let Some(modal) = &mut this.modal
                                                    && modal.id == id
                                                {
                                                    let on = !boolean(&modal.args, "elevated");
                                                    modal.args["elevated"] = json!(on);
                                                    modal.args["failure"] = Value::Null;
                                                }
                                                window.refresh();
                                                cx.notify();
                                            }))
                                        })
                                        .child(
                                            div()
                                                .mt(px(2.))
                                                .size(px(17.))
                                                .flex_shrink_0()
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .rounded(px(4.))
                                                .border_1()
                                                .border_color(if elevated {
                                                    p.accent
                                                } else {
                                                    p.muted
                                                })
                                                .bg(if elevated {
                                                    p.accent
                                                } else {
                                                    p.background.opacity(0.)
                                                })
                                                .when(elevated, |d| {
                                                    d.child(
                                                        icon("CheckIcon", p.on_accent)
                                                            .size(px(12.)),
                                                    )
                                                }),
                                        )
                                        .child(
                                            div()
                                                .flex()
                                                .flex_col()
                                                .child(
                                                    div()
                                                        .flex()
                                                        .items_center()
                                                        .gap(px(8.))
                                                        .child(label("tasks.administrator"))
                                                        .child(
                                                            icon("ShieldIcon", p.muted)
                                                                .size(px(14.)),
                                                        ),
                                                )
                                                .child(
                                                    div()
                                                        .mt(px(3.))
                                                        .min_h(px(36.))
                                                        .text_size(theme::font(12.))
                                                        .line_height(relative(1.5))
                                                        .text_color(p.muted)
                                                        .child(label(if elevated {
                                                            "tasks.administratorHint"
                                                        } else {
                                                            "tasks.standardHint"
                                                        })),
                                                ),
                                        ),
                                )
                                .child(
                                    div()
                                        .min_h(px(62.))
                                        .pt(px(8.))
                                        .text_size(theme::font(12.))
                                        .line_height(relative(1.5))
                                        .text_color(if shown_error { p.danger } else { p.muted })
                                        .flex()
                                        .flex_col()
                                        .when_some(feedback, |d, (message, code)| {
                                            d.child(message).when_some(code, |d, code| {
                                                d.child(
                                                    div().text_size(theme::font(11.)).child(code),
                                                )
                                            })
                                        })
                                        .when(failure_kind.is_empty() && busy, |d| {
                                            d.child(label("tasks.waiting"))
                                        }),
                                ),
                        ),
                ),
            )
            .footer(
                self.measure(id, 1).mr(px(scrollbar::SIZE)).child(
                    self.dialog_footer()
                        .child(
                            self.btn(
                                format!("modal-{id}-cancel"),
                                label("common.cancel"),
                                None,
                                Tone::Plain,
                            )
                            .min_w(px(96.))
                            .inert(pending)
                            .on_click(
                                cx.listener(|this, _, window, cx| this.close_overlay(window, cx)),
                            ),
                        )
                        .child(
                            self.btn(
                                format!("modal-{id}-run"),
                                label(if busy { "tasks.running" } else { "tasks.run" }),
                                None,
                                Tone::Accent,
                            )
                            .min_w(px(96.))
                            .track_focus(&self.primary_focus(id, cx))
                            .inert(pending || !valid_task(&command))
                            .on_click(cx.listener(|this, _, window, cx| this.run_task(window, cx))),
                        ),
                ),
            )
    }
}
