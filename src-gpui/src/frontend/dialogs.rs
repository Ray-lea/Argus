use super::*;
use gpui_kit::component::menu::PopupMenu;
use gpui_kit::component::{ActiveTheme, WindowExt, dialog::Dialog};

#[derive(Clone)]
pub(super) struct Modal {
    pub kind: String,
    pub args: Value,
    pub title: String,
    pub body: String,
    pub error: Option<String>,
}
impl Desktop {
    pub(in crate::frontend) fn open_popup(
        &mut self,
        items: Vec<MenuItem>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let owner = cx.entity().downgrade();
        let focus = self.focus.clone();
        let interval = self.interval;
        let menu = PopupMenu::build(window, cx, move |menu, window, cx| {
            let items = items
                .iter()
                .cloned()
                .map(|item| {
                    let checked = item.checked
                        || matches!(item.action, Action::Interval(ms) if ms == interval);
                    item.checked(checked)
                })
                .collect();
            menus::fill_menu(
                menu.action_context(focus.clone()),
                items,
                owner.clone(),
                window,
                cx,
            )
        });
        let subscription = cx.subscribe_in(&menu, window, |this, menu, _: &DismissEvent, _, cx| {
            if this
                .popup
                .as_ref()
                .is_some_and(|popup| popup.menu.entity_id() == menu.entity_id())
            {
                this.popup = None;
                this.rebuild_rows(false, cx);
                cx.notify();
            }
        });
        menu.focus_handle(cx).focus(window, cx);
        self.popup = Some(Popup {
            menu,
            position: window.mouse_position(),
            _subscription: subscription,
        });
    }
    pub(in crate::frontend) fn open_selected_action(
        &mut self,
        kind: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(row) = self
            .selected
            .get(&self.page)
            .and_then(|key| self.rows.iter().find(|r| &r.key == key))
            .filter(|r| r.section.is_none())
        {
            self.open_dialog(
                kind,
                json!({"members":row.members,"name":row.title}),
                window,
                cx,
            );
        }
    }
    pub(in crate::frontend) fn form(
        &mut self,
        key: &str,
        value: &str,
        placeholder: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let value = value.to_owned();
        let placeholder = label(placeholder);
        self.forms.insert(
            key.into(),
            cx.new(|cx| {
                let mut state = InputState::new(window, cx).placeholder(placeholder);
                state.set_value(value, window, cx);
                state
            }),
        );
    }
    pub(in crate::frontend) fn open_dialog(
        &mut self,
        kind: &str,
        args: Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.popup = None;
        self.forms.clear();
        let name = text(&args, "name");
        let members = values(&args["members"]);
        let pid = members
            .first()
            .map(|p| p["pid"].to_string())
            .unwrap_or_default();
        let (title, body) = match kind {
            "end" => (
                tr!("actions.end.title", name = name),
                if members.len() > 1 {
                    tr!("actions.endGroup.body", name = name, count = members.len())
                } else {
                    tr!("actions.end.body", pid = pid)
                },
            ),
            "endTree" => (
                tr!("actions.endTree.title", name = name),
                tr!("actions.end.body", pid = pid),
            ),
            "priority" => (
                label("processes.menu.setPriority"),
                if text(&args, "priority") == "realtime" {
                    tr!("actions.priority.confirmBody")
                } else {
                    label(&format!("processes.priority.{}", text(&args, "priority")))
                },
            ),
            "efficiency" => (
                label("common.efficiencyMode"),
                label(if boolean(&args, "on") {
                    "common.on"
                } else {
                    "common.off"
                }),
            ),
            "task" => (label("tasks.title"), label("tasks.description")),
            "restartExplorer" => (
                label("tasks.explorer.title"),
                label("tasks.explorer.confirm"),
            ),
            "filelock" => (label("fileLock.title"), label("fileLock.intro")),
            "hotkey" => (
                label("settings.hotkey.title"),
                label("settings.hotkey.desc"),
            ),
            "ruleScope" => (
                label("actions.ruleScope.title"),
                tr!("actions.ruleScope.body", name = name),
            ),
            "rule" => (
                label("processes.menu.addRule"),
                label("settings.rules.desc"),
            ),
            "note" => (label("common.marks.note"), name.into()),
            "privilege" => {
                let uninstall = text(&args, "action") == "uninstall";
                let prefix = if uninstall {
                    "settings.privilege.uninstall"
                } else {
                    "settings.privilege.install"
                };
                (
                    label(&format!("{prefix}.title")),
                    label(&format!("{prefix}.body")),
                )
            }
            "taskmgr" => (
                label("settings.taskManager.confirm.title"),
                label("settings.taskManager.confirm.body"),
            ),
            "sensors" => (
                label("settings.sensors.title"),
                label("settings.sensors.desc"),
            ),
            "clearHistory" => (
                label("settings.data.history.confirm.title"),
                tr!(
                    "settings.data.history.confirm.body",
                    size = theme::bytes(
                        self.data
                            .get("historyUsage")
                            .map(|v| number(v, "bytes"))
                            .unwrap_or(0.),
                        false
                    )
                ),
            ),
            "closeConnection" => (
                label("connections.menu.close"),
                format!(
                    "{}:{} → {}:{}",
                    text(&args, "localAddress"),
                    args["localPort"],
                    text(&args, "remoteAddress"),
                    args["remotePort"]
                ),
            ),
            "proxy" => (
                label("settings.update.custom.title"),
                label("settings.update.custom.desc"),
            ),
            "update" => (
                label("settings.update.restart"),
                label("settings.update.detail.ready"),
            ),
            "license" => (
                label("settings.about.license.title"),
                include_str!("../../../LICENSE").into(),
            ),
            "notices" => (
                label("settings.about.notices.dialog"),
                include_str!("../../../THIRD-PARTY-NOTICES.txt").into(),
            ),
            "shortcuts" => (
                label("settings.shortcuts.title"),
                ["search", "pause", "end", "inputHint"]
                    .map(|key| label(&format!("settings.shortcuts.{key}")))
                    .join("\n\n"),
            ),
            "onboarding" => (
                label("settings.about.onboarding.title"),
                label("settings.about.tagline"),
            ),
            "whatsnew" => (
                tr!(
                    "updates.dialog.installed",
                    version = env!("CARGO_PKG_VERSION")
                ),
                welcome::current_notes(),
            ),
            _ => return,
        };
        match kind {
            "task" => self.form("value", "", "tasks.placeholder", window, cx),
            "filelock" => self.form("value", "", "common.path", window, cx),
            "hotkey" => {
                self.form(
                    "value",
                    text(&args, "hotkey"),
                    "settings.hotkey.recording",
                    window,
                    cx,
                );
                self.request("suspend_hotkey", json!({}), None, window, cx);
            }
            "rule" => self.form("value", name, "settings.rules.placeholder", window, cx),
            "proxy" => self.form(
                "value",
                text(&args, "updateProxy"),
                "settings.update.custom.label",
                window,
                cx,
            ),
            "note" => self.form(
                "value",
                text(&self.marks()[text(&args, "key")], "note"),
                "common.marks.placeholder",
                window,
                cx,
            ),
            _ => {}
        }
        self.modal = Some(Modal {
            kind: kind.into(),
            args,
            title,
            body,
            error: None,
        });
        let owner = cx.entity().downgrade();
        window.close_dialog(cx);
        window.open_dialog(cx, move |dialog, window, cx| {
            if let Some(owner) = owner.upgrade() {
                owner.update(cx, |this, cx| this.dialog_view(dialog, window, cx))
            } else {
                dialog
            }
        });
        if let Some(input) = self.forms.get("value") {
            input.update(cx, |state, cx| state.focus(window, cx));
        }
        cx.notify();
    }
    pub(in crate::frontend) fn dialog_view(
        &self,
        dialog: Dialog,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Dialog {
        let Some(modal) = &self.modal else {
            return dialog;
        };
        if modal.kind == "onboarding" {
            return self.onboarding_dialog(
                dialog.bg(self.palette.layer_strong.opacity(1.)),
                window,
                cx,
            );
        }
        let mut content = div()
            .flex()
            .flex_col()
            .gap_4()
            .when(modal.kind != "whatsnew", |d| d.child(modal.body.clone()))
            .when(modal.kind == "whatsnew", |d| {
                d.child(gpui_kit::component::text::TextView::markdown(
                    "release-notes",
                    modal.body.clone(),
                ))
            });
        if modal.kind == "update" {
            let status = self.data.get("updates").unwrap_or(&Value::Null);
            let current_ver = text(&self.data["info"], "version");
            let next_ver = text(status, "version");
            if !next_ver.is_empty() {
                content = content.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .text_xs()
                        .child(
                            div()
                                .px_2()
                                .py_0p5()
                                .rounded_full()
                                .bg(self.palette.control)
                                .text_color(self.palette.muted)
                                .child(format!(
                                    "{}: {current_ver}",
                                    label("updates.dialog.current")
                                )),
                        )
                        .child("→")
                        .child(
                            div()
                                .px_2()
                                .py_0p5()
                                .rounded_full()
                                .bg(self.palette.accent.opacity(0.16))
                                .text_color(self.palette.accent)
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(format!("{}: {next_ver}", label("updates.dialog.next"))),
                        )
                        .when(text(status, "state") == "ready", |d| {
                            d.child(
                                div()
                                    .px_2()
                                    .py_0p5()
                                    .rounded_full()
                                    .bg(self.palette.cpu.opacity(0.16))
                                    .text_color(self.palette.cpu)
                                    .child(label("updates.dialog.verified")),
                            )
                        }),
                );
            }
            if text(status, "state") == "downloading" {
                let received = number(status, "received");
                let total = number(status, "total");
                let ratio = if total > 0. {
                    (received / total).clamp(0., 1.) as f32
                } else {
                    0.
                };
                content = content.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(div().text_xs().text_color(self.palette.muted).child(tr!(
                            "updates.dialog.progress",
                            received = theme::bytes(received, false),
                            total = theme::bytes(total, false)
                        )))
                        .child(
                            div()
                                .w_full()
                                .h_2()
                                .rounded_full()
                                .overflow_hidden()
                                .bg(self.palette.stroke)
                                .child(div().h_full().w(relative(ratio)).bg(self.palette.accent)),
                        ),
                );
            }
            content = content.child(gpui_kit::component::text::TextView::markdown(
                "update-release-notes",
                welcome::update_notes(status),
            ));
            if let Some(error) = status["error"].as_str() {
                content = content.child(
                    div()
                        .text_color(self.palette.danger)
                        .child(error.to_owned()),
                );
            }
        }
        if let Some(error) = &modal.error {
            content = content.child(div().text_color(self.palette.danger).child(error.clone()));
        }
        if let Some(input) = self.forms.get("value") {
            if modal.kind == "hotkey" {
                content = content.child(
                    div()
                        .id("hotkey-recorder")
                        .track_focus(&input.read(cx).focus_handle(cx))
                        .p_4()
                        .border_1()
                        .border_color(self.palette.stroke)
                        .child(input.read(cx).value().to_string())
                        .capture_key_down(cx.listener(Self::record_hotkey))
                        .on_key_up(cx.listener(|this, _: &KeyUpEvent, window, cx| {
                            if this
                                .modal
                                .as_ref()
                                .is_some_and(|m| m.args["recorded"].as_bool() == Some(true))
                            {
                                this.confirm_dialog(window, cx);
                            }
                        })),
                );
            } else {
                content = content.child(Input::new(input));
            }
        }
        if modal.kind == "task" {
            content = content
                .child(self.button(
                    "task-browse",
                    label("tasks.browse"),
                    None,
                    Action::Invoke("pick_path".into(), json!({"folder":false,"task":true})),
                    false,
                    cx,
                ))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(label("tasks.administrator"))
                        .child(self.switch(
                            "task-elevated",
                            label("tasks.administrator"),
                            boolean(&modal.args, "elevated"),
                            Action::Preference(
                                "dialog.elevated".into(),
                                json!(!boolean(&modal.args, "elevated")),
                            ),
                            cx,
                        )),
                );
        }
        if modal.kind == "filelock" {
            content = content.child(div().flex().gap_2().children(
                [(false, "fileLock.pickFile"), (true, "fileLock.pickFolder")].map(
                    |(folder, key)| {
                        self.button(
                            key,
                            label(key),
                            None,
                            Action::Invoke("pick_path".into(), json!({"folder":folder})),
                            false,
                            cx,
                        )
                    },
                ),
            ));
            if let Some(report) = self.data.get("lockers") {
                let lockers = values(&report["lockers"]);
                content = content.child(if lockers.is_empty() {
                    label("fileLock.none")
                } else {
                    tr!("fileLock.found", count = lockers.len())
                });
                for locker in lockers {
                    let kind = text(locker, "kind");
                    let handles_count = locker["handleCount"].as_u64().unwrap_or(0);
                    let svc_name = text(locker, "serviceName");
                    content = content.child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .p_2()
                            .rounded_md()
                            .border_1()
                            .border_color(self.palette.stroke)
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .min_w_0()
                                    .flex_1()
                                    .child(icon("AppIcon", self.palette.muted).size_4())
                                    .child(div().truncate().font_weight(FontWeight::MEDIUM).child(
                                        format!(
                                            "{} · PID {}",
                                            text(locker, "appName"),
                                            locker["pid"]
                                        ),
                                    ))
                                    .when(!kind.is_empty() && kind != "unknown", |d| {
                                        d.child(
                                            div()
                                                .px_1p5()
                                                .rounded_full()
                                                .text_xs()
                                                .bg(self.palette.control)
                                                .text_color(self.palette.muted)
                                                .child(label(&format!("fileLock.kind.{kind}"))),
                                        )
                                    })
                                    .when(!svc_name.is_empty(), |d| {
                                        d.child(
                                            div()
                                                .text_xs()
                                                .text_color(self.palette.muted)
                                                .child(tr!("fileLock.service", name = svc_name)),
                                        )
                                    })
                                    .when(handles_count > 0, |d| {
                                        d.child(
                                            div().text_xs().text_color(self.palette.muted).child(
                                                tr!("fileLock.handleCount", count = handles_count),
                                            ),
                                        )
                                    }),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .flex_shrink_0()
                                    .child(
                                        self.button(
                                            format!("locker-{}", model::identity(locker)),
                                            label("fileLock.showProcess"),
                                            None,
                                            Action::Preference(
                                                "revealProcess".into(),
                                                locker.clone(),
                                            ),
                                            false,
                                            cx,
                                        )
                                        .min_h_0()
                                        .h_6()
                                        .text_xs(),
                                    )
                                    .child(
                                        self.button(
                                            format!("locker-end-{}", model::identity(locker)),
                                            label("fileLock.end"),
                                            None,
                                            Action::Invoke(
                                                "kill_process".into(),
                                                json!({
                                                    "pid": locker["pid"],
                                                    "createTime": locker["createTime"],
                                                    "force": false
                                                }),
                                            ),
                                            false,
                                            cx,
                                        )
                                        .min_h_0()
                                        .h_6()
                                        .text_xs()
                                        .text_color(self.palette.danger),
                                    ),
                            ),
                    );
                }
                if report["handles"]["denied"].as_u64().unwrap_or(0) > 0 {
                    content = content.child(
                        div()
                            .text_xs()
                            .text_color(self.palette.muted)
                            .child(label("fileLock.serviceHint")),
                    );
                }
            }
        }
        if modal.kind == "rule" {
            for (field, options) in [
                (
                    "priority",
                    vec![
                        "idle",
                        "belowNormal",
                        "normal",
                        "aboveNormal",
                        "high",
                        "realtime",
                    ],
                ),
                ("efficiency", vec!["keep", "on", "off"]),
            ] {
                let mut choices = div().flex().flex_wrap().gap_1();
                for value in options {
                    let label = if field == "priority" {
                        label(&format!("processes.priority.{value}"))
                    } else {
                        label(&format!("settings.rules.efficiency.{value}"))
                    };
                    choices = choices.child(self.button(
                        format!("rule-{field}-{value}"),
                        label,
                        None,
                        Action::Preference(format!("dialog.{field}"), json!(value)),
                        text(&modal.args, field) == value,
                        cx,
                    ));
                }
                content = content.child(choices);
            }
        }
        let owner = cx.entity().downgrade();
        let confirm_owner = owner.clone();
        dialog
            .bg(self.palette.layer_strong.opacity(1.))
            .title(modal.title.clone())
            .overlay_closable(false)
            .child(content)
            .on_ok(move |_, window, cx| {
                let _ = confirm_owner.update(cx, |this, cx| this.confirm_dialog(window, cx));
                false
            })
            .footer(
                div()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .child(
                        self.button(
                            "dialog-cancel",
                            label("common.cancel"),
                            None,
                            Action::CloseOverlay,
                            false,
                            cx,
                        )
                        .border_1()
                        .border_color(self.palette.stroke)
                        .bg(self.palette.control),
                    )
                    .child(
                        self.button(
                            "dialog-confirm",
                            label(match modal.kind.as_str() {
                                "task" => "tasks.run",
                                "update" => {
                                    if self
                                        .data
                                        .get("updates")
                                        .is_some_and(|s| text(s, "state") == "ready")
                                    {
                                        "settings.update.restart"
                                    } else {
                                        "settings.update.download"
                                    }
                                }
                                "filelock" => "fileLock.recheck",
                                "end" | "endTree" => "common.endProcess",
                                "license" | "notices" | "shortcuts" | "onboarding" | "whatsnew" => {
                                    "common.close"
                                }
                                _ => "common.marks.save",
                            }),
                            None,
                            Action::Confirm,
                            true,
                            cx,
                        )
                        .bg(if matches!(modal.kind.as_str(), "end" | "endTree") {
                            self.palette.danger
                        } else {
                            self.palette.accent
                        })
                        .text_color(cx.theme().primary_foreground)
                        .min_w_20(),
                    ),
            )
            .on_close(move |_, window, cx| {
                let _ = owner.update(cx, |this, cx| {
                    this.modal = None;
                    this.request("resume_hotkey", json!({}), Some("settings"), window, cx);
                    cx.notify();
                });
            })
    }
    pub(in crate::frontend) fn confirm_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(modal) = self.modal.clone() else {
            return;
        };
        let value = self
            .forms
            .get("value")
            .map(|v| v.read(cx).value().to_string())
            .unwrap_or_default();
        let args = &modal.args;
        match modal.kind.as_str() {
            "restartExplorer" => self.request(
                "restart_explorer",
                json!({"pid":args["pid"],"createTime":args["createTime"]}),
                None,
                window,
                cx,
            ),
            "end" | "endTree" | "priority" | "efficiency" => {
                for member in values(&args["members"]) {
                    let mut payload =
                        json!({"pid":member["pid"],"createTime":member["createTime"]});
                    let command = match modal.kind.as_str() {
                        "end" => {
                            payload["force"] = json!(false);
                            "kill_process"
                        }
                        "endTree" => "kill_process_tree",
                        "priority" => {
                            payload["priority"] = args["priority"].clone();
                            "set_priority"
                        }
                        _ => {
                            payload["on"] = args["on"].clone();
                            "set_efficiency_mode"
                        }
                    };
                    self.request(command, payload, None, window, cx);
                }
            }
            "task" => {
                self.request(
                    "run_task",
                    json!({"command":value,"elevated":boolean(args,"elevated")}),
                    None,
                    window,
                    cx,
                );
                return;
            }
            "filelock" => {
                let paths = value
                    .lines()
                    .filter(|p| !p.trim().is_empty())
                    .map(str::trim)
                    .collect::<Vec<_>>();
                if paths.is_empty() {
                    return;
                }
                self.request(
                    "find_lockers",
                    json!({"paths":paths}),
                    Some("lockers"),
                    window,
                    cx,
                );
                return;
            }
            "hotkey" => self.request(
                "set_hotkey",
                json!({"hotkey":value}),
                Some("settings"),
                window,
                cx,
            ),
            "privilege" => self.request(
                "manage_privileged_service",
                json!({"action":args["action"]}),
                Some("privilege"),
                window,
                cx,
            ),
            "taskmgr" => self.request(
                "manage_task_manager",
                json!({"action":args["action"]}),
                Some("taskmgr"),
                window,
                cx,
            ),
            "sensors" => self.request(
                "set_kernel_sensors",
                json!({"enabled":args["enabled"]}),
                Some("settings"),
                window,
                cx,
            ),
            "clearHistory" => self.request("clear_history", json!({}), None, window, cx),
            "closeConnection" => self.request("close_connection", args.clone(), None, window, cx),
            "update" => {
                let status = self.data.get("updates").cloned().unwrap_or(Value::Null);
                match text(&status, "state") {
                    "ready" => self.request("install_update", json!({}), None, window, cx),
                    "available" => self.request(
                        "open_release_page",
                        json!({"version":status["version"]}),
                        None,
                        window,
                        cx,
                    ),
                    _ => return,
                }
            }
            "proxy" => self.preference("updateProxy", json!(value), window, cx),
            "note" => self.set_mark(
                text(args, "key"),
                "note",
                json!(
                    value
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ")
                        .chars()
                        .take(80)
                        .collect::<String>()
                ),
                cx,
            ),
            "ruleScope" => {
                self.open_dialog("rule", args.clone(), window, cx);
                return;
            }
            "rule" => {
                if value.trim().is_empty() {
                    return;
                }
                let mut rules = values(&self.data["settings"]["rules"]).to_vec();
                let id = args["id"].as_str().map(str::to_owned).unwrap_or_else(|| {
                    format!(
                        "native-{}",
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_nanos()
                    )
                });
                rules.retain(|r| r["id"] != id);
                rules.push(json!({"id":id,"name":value.trim(),"enabled":args["enabled"].as_bool().unwrap_or(true),"priority":args["priority"],"efficiency":args["efficiency"].as_bool().or_else(||match text(args,"efficiency"){"on"=>Some(true),"off"=>Some(false),_=>None})}));
                self.request(
                    "set_rules",
                    json!({"rules":rules}),
                    Some("settings"),
                    window,
                    cx,
                );
            }
            _ => {}
        }
        self.act(Action::CloseOverlay, window, cx);
    }
    pub(in crate::frontend) fn command_completed(
        &mut self,
        command: &str,
        value: &Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match command {
            "complete_onboarding" => self.welcome_checked = false,
            "pick_path" => {
                if let Some(path) = value.as_str()
                    && let Some(input) = self.forms.get("value")
                {
                    let value = if self.modal.as_ref().is_some_and(|m| m.kind == "task") {
                        format!("\"{path}\"")
                    } else {
                        path.into()
                    };
                    input.update(cx, |state, cx| state.set_value(value, window, cx));
                }
            }
            "run_task" => {
                self.act(Action::CloseOverlay, window, cx);
                self.toast = Some((label("tasks.started"), Instant::now()));
            }
            "set_theme" => {
                let saved = self.host.state::<crate::settings::SettingsStore>().get();
                self.set_theme(crate::platform::is_dark(saved.theme), window, cx);
            }
            "privileged_service_status" => {
                self.data.insert("privilege".into(), value.clone());
            }
            "kill_process" => {
                if self.modal.as_ref().is_some_and(|m| m.kind == "filelock") {
                    self.confirm_dialog(window, cx);
                }
            }
            "control_service"
            | "set_service_start_type"
            | "set_startup_enabled"
            | "close_connection" => self.poll_page(window, cx),
            _ => {}
        }
    }
    pub(in crate::frontend) fn render_overlays(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let mut elements = vec![];
        if let Some(popup) = &self.popup {
            elements.push(
                deferred(
                    anchored().child(
                        div()
                            .w(window.bounds().size.width)
                            .h(window.bounds().size.height)
                            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                            .child(
                                anchored()
                                    .position(popup.position)
                                    .snap_to_window_with_margin(window.rem_size() / 2.)
                                    .child(popup.menu.clone()),
                            ),
                    ),
                )
                .with_priority(gpui_kit::base::POPUP_PRIORITY)
                .into_any_element(),
            );
        }
        if let Some((text, _)) = &self.toast {
            elements.push(
                div()
                    .absolute()
                    .bottom_4()
                    .right_4()
                    .max_w_96()
                    .bg(self.palette.layer_strong)
                    .text_color(self.palette.ink)
                    .border_1()
                    .border_color(self.palette.stroke)
                    .rounded(cx.theme().radius)
                    .p_4()
                    .child(text.clone())
                    .into_any_element(),
            );
        }
        elements
    }
    fn record_hotkey(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        let key = event.keystroke.key.as_str();
        if event.is_held || matches!(key, "control" | "ctrl" | "alt" | "shift" | "cmd" | "win") {
            return;
        }
        if key == "escape"
            && !event.keystroke.modifiers.control
            && !event.keystroke.modifiers.alt
            && !event.keystroke.modifiers.platform
        {
            self.act(Action::CloseOverlay, window, cx);
            return;
        }
        if key == "backspace" {
            self.request(
                "set_hotkey_enabled",
                json!({"enabled":false}),
                Some("settings"),
                window,
                cx,
            );
            self.act(Action::CloseOverlay, window, cx);
            return;
        }
        let mods = event.keystroke.modifiers;
        let mut parts = Vec::new();
        for (enabled, name) in [
            (mods.control, "Ctrl"),
            (mods.alt, "Alt"),
            (mods.shift, "Shift"),
            (mods.platform, "Win"),
        ] {
            if enabled {
                parts.push(name.to_owned());
            }
        }
        parts.push(match key {
            "escape" => "Escape".into(),
            "enter" => "Enter".into(),
            "space" => "Space".into(),
            "backspace" => "Backspace".into(),
            "pageup" => "PageUp".into(),
            "pagedown" => "PageDown".into(),
            other => other.to_ascii_uppercase(),
        });
        if let Some(input) = self.forms.get("value") {
            input.update(cx, |state, cx| state.set_value(parts.join("+"), window, cx));
        }
        if let Some(modal) = &mut self.modal {
            modal.args["recorded"] = json!(true);
        }
        window.refresh();
    }
}
