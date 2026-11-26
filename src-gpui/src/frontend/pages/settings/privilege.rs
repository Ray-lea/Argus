//! Privileges: running as administrator, the privileged service, the kernel
//! sensors and the task manager replacement, with what each card offers.
use super::*;
use crate::frontend::widgets::{ButtonExt, ToastKind, Tone, service_connected, service_state};

/// What the administrator card shows: locale keys for its state and notes,
/// the state's tone and its controls.
#[derive(Debug, PartialEq)]
pub(in crate::frontend) struct AdministratorView {
    pub state: &'static str,
    pub tone: &'static str,
    pub notes: Vec<&'static str>,
    /// Whether "Repair authorization" is offered.
    pub repair: bool,
    pub checked: bool,
    pub disabled: bool,
}

/// `status` is the host's `admin::Status` (none before it arrives or after it
/// failed); `loading` while it is being read, `busy` while a change runs.
pub(in crate::frontend) fn administrator_view(
    status: Option<&Value>,
    loading: bool,
    busy: bool,
    onboarding: bool,
) -> AdministratorView {
    let flag = |key: &str| status.is_some_and(|s| boolean(s, key));
    let (supported, enabled, ready, eligible) = (
        flag("supported"),
        flag("enabled"),
        flag("ready"),
        flag("eligible"),
    );
    let failed = status.is_some_and(|s| s["error"].is_string());
    let working = busy || loading;
    let state = if working {
        "settings.administrator.busy"
    } else if enabled && ready {
        "settings.administrator.authorized"
    } else if enabled {
        "settings.administrator.repair"
    } else {
        "settings.administrator.off"
    };
    let tone = if working {
        "busy"
    } else if ready {
        "ok"
    } else if enabled {
        "warn"
    } else {
        "off"
    };
    let mut notes = vec![
        if flag("elevated") {
            "settings.administrator.currentAdmin"
        } else {
            "settings.administrator.currentStandard"
        },
        if onboarding {
            "settings.administrator.afterOnboarding"
        } else {
            "settings.administrator.restart"
        },
    ];
    if status.is_some() && !supported {
        notes.push("settings.administrator.development");
    }
    if supported && !eligible {
        notes.push("settings.administrator.standardAccount");
    }
    AdministratorView {
        state,
        tone,
        notes,
        repair: supported && enabled && (!ready || failed),
        checked: enabled,
        disabled: working || !eligible || (!supported && !enabled),
    }
}

impl Desktop {
    // ---- Privileges -----------------------------------------------------------

    pub(super) fn service_card(&self, cx: &mut Context<Self>) -> AnyElement {
        let status = self.data.get("privilege").cloned().unwrap_or(Value::Null);
        let busy = self.ui.busy.contains("privilege");
        let checking = !self.data.contains_key("privilege");
        let state = if busy {
            label("settings.privilege.state.busy")
        } else if checking {
            label("settings.privilege.state.checking")
        } else {
            service_state(&status)
        };
        let tone = if busy || checking {
            "busy"
        } else if text(&status, "state") == "notInstalled" {
            "off"
        } else if service_connected(&status) {
            "ok"
        } else {
            "warn"
        };
        let app = text(&self.data["info"], "version");
        let outdated = status["version"]
            .as_str()
            .filter(|v| !app.is_empty() && *v != app)
            .map(|v| tr!("settings.privilege.outdated", service = v, app = app));
        let service_error = if boolean(&status, "incompatible") {
            Some(label("settings.privilege.incompatible"))
        } else {
            status["error"].as_str().map(str::to_owned)
        };
        let error = self
            .ui
            .errors
            .get("privilege")
            .cloned()
            .or(service_error)
            .or(outdated)
            // The state could not be read at all.
            .or_else(|| self.ui.errors.get("privilegeStatus").cloned());
        // What brings a damaged installation back, where this account may:
        // an update, or a repair that authorizes anew and asks first.
        let state_name = text(&status, "state");
        let repair = match (state_name, status["repair"].as_str()) {
            ("broken", Some("update")) => Some("update"),
            ("broken", Some("repair")) => Some("repair"),
            _ => None,
        };
        let note = label(match state_name {
            "broken" if repair.is_some() => "settings.privilege.broken.repair",
            "broken" => "settings.privilege.broken.remove",
            "deletePending" => "settings.privilege.deletePending",
            _ => "settings.privilege.statusDesc",
        });
        let action =
            |this: &Desktop, id: &str, key: &str, act: &'static str, cx: &mut Context<Desktop>| {
                this.btn(format!("privilege-{id}"), label(key), None, Tone::Plain)
                    .inert(busy || checking)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.manage_service(act.into(), window, cx);
                    }))
                    .into_any_element()
            };
        let mut controls = vec![
            self.btn(
                "privilege-refresh",
                label("settings.refreshStatus"),
                None,
                Tone::Plain,
            )
            .inert(busy || checking)
            .on_click(cx.listener(|this, _, window, cx| {
                this.ui.errors.remove("privilege");
                this.ui.errors.remove("privilegeStatus");
                this.data.remove("privilege");
                this.request(
                    "privileged_service_status",
                    json!({}),
                    Some("privilege"),
                    window,
                    cx,
                );
            }))
            .into_any_element(),
        ];
        if !status.is_null() {
            if state_name == "notInstalled" {
                controls.push(action(
                    self,
                    "install",
                    "settings.privilege.install.action",
                    "install",
                    cx,
                ));
            } else if state_name == "broken" {
                // Starting or updating cannot work on what is left; removing
                // the registration always can.
                if let Some(repair) = repair {
                    controls.push(action(
                        self,
                        "repair",
                        "settings.privilege.repair.action",
                        repair,
                        cx,
                    ));
                }
                controls.push(action(
                    self,
                    "uninstall",
                    "settings.privilege.uninstall.action",
                    "uninstall",
                    cx,
                ));
            } else if state_name != "deletePending" {
                if boolean(&status, "authorized") && text(&status, "state") == "stopped" {
                    controls.push(action(
                        self,
                        "start",
                        "settings.privilege.start",
                        "start",
                        cx,
                    ));
                }
                if boolean(&status, "authorized") {
                    controls.push(action(
                        self,
                        "update",
                        "settings.privilege.update",
                        "update",
                        cx,
                    ));
                }
                controls.push(action(
                    self,
                    "uninstall",
                    "settings.privilege.uninstall.action",
                    "uninstall",
                    cx,
                ));
            }
        }
        self.feature(
            "privilegedService",
            "ShieldIcon",
            label("common.privilegedService"),
            label("settings.privilege.desc"),
            state,
            tone,
            note.into_any_element(),
            error,
            controls,
            None,
        )
    }

    /// Installs, starts, updates, repairs or removes the service. Installing
    /// and removing ask first, and so does a repair: it authorizes this
    /// account anew, which is confirmed with what an installation grants.
    pub(in crate::frontend) fn manage_service(
        &mut self,
        action: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.ui.busy.contains("privilege") {
            return;
        }
        let (title, body, confirm) = match action.as_str() {
            "install" => (
                label("settings.privilege.install.title"),
                label("settings.privilege.install.body"),
                "settings.privilege.install.action",
            ),
            "repair" => (
                label("settings.privilege.repair.title"),
                format!(
                    "{}\n\n{}",
                    label("settings.privilege.repair.body"),
                    label("settings.privilege.install.body")
                ),
                "settings.privilege.repair.confirm",
            ),
            "uninstall" => (
                label("settings.privilege.uninstall.title"),
                label("settings.privilege.uninstall.body"),
                "settings.privilege.uninstall.confirm",
            ),
            _ => {
                self.service_action(action, window, cx);
                return;
            }
        };
        let danger = action == "uninstall";
        let run = move |this: &mut Desktop, window: &mut Window, cx: &mut Context<Desktop>| {
            this.service_action(action.clone(), window, cx)
        };
        self.ask(title, body, label(confirm), danger, run, window, cx);
    }

    /// Says once per run that the service is registered but damaged, with a
    /// way to its card. A crash or an update that was cut short leaves it so,
    /// and Windows then fails to start it at every boot until it is repaired
    /// or removed. The onboarding shows the state itself.
    pub(in crate::frontend) fn notice_service_damage(&mut self, status: &Value) {
        if text(status, "state") != "broken"
            || self.ui.errors.contains_key("privilegeDamageNoticed")
            || self
                .modal
                .iter()
                .chain(&self.ui.below)
                .any(|modal| modal.kind == "onboarding")
        {
            return;
        }
        self.ui
            .errors
            .insert("privilegeDamageNoticed".into(), "1".into());
        self.toast_with(
            ToastKind::Error,
            label("settings.privilege.broken.notice"),
            12000,
            vec![(
                label("settings.privilege.broken.view"),
                Rc::new(
                    |this: &mut Desktop, window: &mut Window, cx: &mut Context<Desktop>| {
                        this.locate_setting("privilege", "privilegedService", window, cx)
                    },
                ),
            )],
        );
    }

    /// Runs a service action without asking (the onboarding acts in place).
    pub(in crate::frontend) fn service_action(
        &mut self,
        action: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.ui.busy.contains("privilege") || self.options.fixture.is_some() {
            return;
        }
        self.ui.busy.insert("privilege".into());
        self.ui.errors.remove("privilege");
        self.ui
            .errors
            .insert("privilegeAction".into(), action.clone());
        self.request(
            "manage_privileged_service",
            json!({"action":action}),
            None,
            window,
            cx,
        );
    }

    pub(super) fn sensors_card(
        &self,
        settings: &Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.palette;
        let enabled = boolean(settings, "kernelSensors");
        let busy = self.ui.busy.contains("sensors");
        let status = if busy {
            label("settings.sensors.preparing")
        } else if enabled {
            argus_core::i18n::text(&format!(
                "performance.cpu.sensorState.{}",
                self.snapshot["hardware"]["cpuSensors"]["state"]
                    .as_str()
                    .unwrap_or("pending")
            ))
        } else {
            label("settings.sensors.off")
        };
        let driver = self
            .btn(
                "sensor-driver",
                label("settings.sensors.driver"),
                Some("ExternalIcon"),
                Tone::Plain,
            )
            .inert(busy)
            .on_click(cx.listener(|this, _, window, cx| {
                this.request("open_sensor_driver_page", json!({}), None, window, cx);
            }))
            .into_any_element();
        let switch = self.toggle(
            "kernel-sensors",
            label("settings.sensors.title"),
            enabled,
            busy,
            |this, on, window, cx| this.set_sensors(on, window, cx),
            window,
            cx,
        );
        self.setting(
            "kernelSensors",
            self.glyph("BoltIcon"),
            label("settings.sensors.title"),
            Some(
                div()
                    .flex()
                    .flex_col()
                    .child(label("settings.sensors.desc"))
                    .child(div().text_color(p.muted).child(status))
                    .into_any_element(),
            ),
            self.ui.errors.get("sensors").cloned(),
            vec![driver, switch],
        )
    }

    pub(in crate::frontend) fn set_sensors(
        &mut self,
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.ui.busy.contains("sensors") || self.options.fixture.is_some() {
            return;
        }
        self.ui.busy.insert("sensors".into());
        self.ui.errors.remove("sensors");
        self.request(
            "set_kernel_sensors",
            json!({"enabled":enabled}),
            None,
            window,
            cx,
        );
    }

    pub(super) fn task_manager_card(&self, cx: &mut Context<Self>) -> AnyElement {
        let status = self.data.get("taskmgr").cloned().unwrap_or(Value::Null);
        let busy = self.ui.busy.contains("taskmgr");
        let opening = self.ui.busy.contains("nativeTaskmgr");
        let checking = !self.data.contains_key("taskmgr");
        let state_name = text(&status, "state");
        let state = if busy {
            label("settings.taskManager.busy")
        } else if checking {
            label("settings.taskManager.checking")
        } else if status.is_null() {
            label("settings.taskManager.unknown")
        } else {
            label(&format!("settings.taskManager.state.{state_name}"))
        };
        let tone = if busy || checking {
            "busy"
        } else if state_name == "enabled" {
            "ok"
        } else if state_name == "disabled" {
            "off"
        } else {
            "warn"
        };
        let mut controls = vec![
            self.btn(
                "taskmgr-refresh",
                label("settings.refreshStatus"),
                None,
                Tone::Plain,
            )
            .inert(busy || checking || opening)
            .on_click(cx.listener(|this, _, window, cx| {
                this.ui.errors.remove("taskmgr");
                this.data.remove("taskmgr");
                this.request(
                    "task_manager_status",
                    json!({}),
                    Some("taskmgr"),
                    window,
                    cx,
                );
            }))
            .into_any_element(),
        ];
        if boolean(&status, "canEnable") {
            controls.push(
                self.btn(
                    "taskmgr-enable",
                    label("settings.taskManager.enable"),
                    None,
                    Tone::Plain,
                )
                .inert(busy || checking || opening)
                .on_click(cx.listener(|this, _, window, cx| {
                    this.manage_task_manager("enable".into(), window, cx)
                }))
                .into_any_element(),
            );
        }
        if boolean(&status, "canRestore") {
            controls.push(
                self.btn(
                    "taskmgr-restore",
                    label("settings.taskManager.restore"),
                    None,
                    Tone::Plain,
                )
                .inert(busy || checking || opening)
                .on_click(cx.listener(|this, _, window, cx| {
                    this.manage_task_manager("restore".into(), window, cx)
                }))
                .into_any_element(),
            );
        }
        let open_label = label(if opening {
            "settings.taskManager.openingNative"
        } else {
            "settings.taskManager.openNative"
        });
        controls.push(
            self.btn("taskmgr-open-native", "", None, Tone::Plain)
                .accessibility_label(open_label.clone())
                .child(div().flex_shrink_0().child(self.glyph("ExternalLinkIcon")))
                .child(div().min_w_0().whitespace_normal().child(open_label))
                .max_w_full()
                .h_auto()
                .min_h(px(32.))
                .py(px(6.))
                .whitespace_normal()
                .inert(busy || opening)
                .tooltip(|window, cx| {
                    widgets::tooltip(label("settings.taskManager.openNativeHint"), window, cx)
                })
                .on_click(cx.listener(|this, _, window, cx| {
                    if this.options.fixture.is_some()
                        || this.ui.busy.contains("taskmgr")
                        || !this.ui.busy.insert("nativeTaskmgr".into())
                    {
                        return;
                    }
                    this.ui.errors.remove("taskmgr");
                    this.request("open_native_task_manager", json!({}), None, window, cx);
                }))
                .into_any_element(),
        );
        let note = status["detail"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| label("settings.taskManager.retry"));
        self.feature(
            "taskManager",
            "KeyboardIcon",
            label("settings.taskManager.title"),
            label("settings.taskManager.desc"),
            state,
            tone,
            note.into_any_element(),
            self.ui.errors.get("taskmgr").cloned(),
            controls,
            boolean(&status, "canRestore").then(|| {
                tr!(
                    "settings.taskManager.recovery",
                    path = text(&status, "recoveryPath")
                )
            }),
        )
    }

    /// Enables the replacement after asking; restoring asks nothing.
    pub(in crate::frontend) fn manage_task_manager(
        &mut self,
        action: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.ui.busy.contains("taskmgr") {
            return;
        }
        let enable = action == "enable";
        let run = move |this: &mut Desktop, window: &mut Window, cx: &mut Context<Desktop>| {
            this.task_manager_action(action.clone(), window, cx)
        };
        if enable {
            self.ask(
                label("settings.taskManager.confirm.title"),
                label("settings.taskManager.confirm.body"),
                label("settings.taskManager.enable"),
                false,
                run,
                window,
                cx,
            );
        } else {
            run(self, window, cx);
        }
    }

    /// Enables or restores the replacement without asking.
    pub(in crate::frontend) fn task_manager_action(
        &mut self,
        action: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.ui.busy.contains("taskmgr")
            || self.ui.busy.contains("nativeTaskmgr")
            || self.options.fixture.is_some()
        {
            return;
        }
        self.ui.busy.insert("taskmgr".into());
        self.ui.errors.remove("taskmgr");
        self.ui
            .errors
            .insert("taskmgrAction".into(), action.clone());
        self.request(
            "manage_task_manager",
            json!({"action":action}),
            None,
            window,
            cx,
        );
    }

    /// Running Argus as administrator through the one-time grant
    ///; the onboarding shows the same
    /// card and defers the restart to its end.
    pub(in crate::frontend) fn administrator_card(
        &self,
        onboarding: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let status = self.data.get("administrator");
        let busy = self.ui.busy.contains("administrator");
        let loading = (status.is_none() && !self.ui.errors.contains_key("administratorStatus"))
            || self.ui.busy.contains("administratorStatus");
        let view = administrator_view(status, loading, busy, onboarding);
        let note = div()
            .flex()
            .flex_col()
            .children(view.notes.iter().map(|key| div().child(label(key))))
            .into_any_element();
        let error = self
            .ui
            .errors
            .get("administrator")
            .cloned()
            .or_else(|| status.and_then(|s| s["error"].as_str()).map(str::to_owned))
            .or_else(|| self.ui.errors.get("administratorStatus").cloned());
        let mut controls = vec![];
        if view.repair {
            controls.push(
                self.btn(
                    "administrator-repair",
                    label("settings.administrator.repair"),
                    None,
                    Tone::Plain,
                )
                .inert(busy)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.set_administrator(true, true, onboarding, window, cx)
                }))
                .into_any_element(),
            );
        }
        controls.push(self.toggle(
            if onboarding {
                "onboarding-administrator"
            } else {
                "administrator"
            },
            label("settings.administrator.allow"),
            view.checked,
            view.disabled,
            move |this, on, window, cx| this.set_administrator(on, false, onboarding, window, cx),
            window,
            cx,
        ));
        self.feature(
            "administrator",
            "ShieldIcon",
            label("settings.administrator.title"),
            label("settings.administrator.desc"),
            label(view.state),
            view.tone,
            note,
            error,
            controls,
            None,
        )
    }

    /// Turns the administrator start on or off, or repairs it. Outside the
    /// onboarding the host restarts Argus at once with the new permissions.
    fn set_administrator(
        &mut self,
        enabled: bool,
        repair: bool,
        onboarding: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.ui.busy.contains("administrator") || self.options.fixture.is_some() {
            return;
        }
        self.ui.busy.insert("administrator".into());
        self.ui.errors.remove("administrator");
        self.request(
            "set_administrator_mode",
            json!({"enabled":enabled,"defer":onboarding,"repair":repair}),
            None,
            window,
            cx,
        );
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::{AdministratorView, administrator_view};
    use serde_json::{Value, json};

    fn status(enabled: bool, ready: bool, elevated: bool, error: Option<&str>) -> Value {
        json!({
            "supported": true,
            "enabled": enabled,
            "ready": ready,
            "elevated": elevated,
            "eligible": true,
            "error": error,
        })
    }

    #[test]
    fn administrator_card_follows_the_grant() {
        // Off: the switch is free, nothing to repair.
        let off = administrator_view(
            Some(&status(false, false, false, None)),
            false,
            false,
            false,
        );
        assert_eq!(
            off,
            AdministratorView {
                state: "settings.administrator.off",
                tone: "off",
                notes: vec![
                    "settings.administrator.currentStandard",
                    "settings.administrator.restart"
                ],
                repair: false,
                checked: false,
                disabled: false,
            }
        );
        // Authorized and running elevated.
        let on = administrator_view(Some(&status(true, true, true, None)), false, false, false);
        assert_eq!(
            (on.state, on.tone, on.repair, on.checked, on.disabled),
            (
                "settings.administrator.authorized",
                "ok",
                false,
                true,
                false
            )
        );
        assert_eq!(on.notes[0], "settings.administrator.currentAdmin");
        // Enabled but not ready: the state asks for a repair, which is offered.
        let broken =
            administrator_view(Some(&status(true, false, false, None)), false, false, false);
        assert_eq!(
            (broken.state, broken.tone, broken.repair),
            ("settings.administrator.repair", "warn", true)
        );
        // Ready, but this start fell back: repair stays offered.
        let fallback = administrator_view(
            Some(&status(true, true, false, Some("fallback"))),
            false,
            false,
            false,
        );
        assert_eq!(
            (fallback.state, fallback.tone, fallback.repair),
            ("settings.administrator.authorized", "ok", true)
        );
    }

    #[test]
    fn administrator_card_waits_and_explains() {
        // Before the status arrives, and while a change runs.
        let loading = administrator_view(None, true, false, false);
        assert_eq!(
            (
                loading.state,
                loading.tone,
                loading.disabled,
                loading.checked
            ),
            ("settings.administrator.busy", "busy", true, false)
        );
        let busy = administrator_view(Some(&status(false, false, false, None)), false, true, true);
        assert_eq!(
            (busy.state, busy.disabled),
            ("settings.administrator.busy", true)
        );
        assert_eq!(busy.notes[1], "settings.administrator.afterOnboarding");
        // A development build: off and locked, with the reason.
        let mut development = status(false, false, false, None);
        development["supported"] = json!(false);
        let view = administrator_view(Some(&development), false, false, false);
        assert!(view.disabled);
        assert!(view.notes.contains(&"settings.administrator.development"));
        // An enabled grant can still be turned off there, but not repaired.
        development["enabled"] = json!(true);
        let view = administrator_view(Some(&development), false, false, false);
        assert!(!view.disabled && !view.repair);
        // An account without an administrator identity.
        let mut standard = status(false, false, false, None);
        standard["eligible"] = json!(false);
        let view = administrator_view(Some(&standard), false, false, false);
        assert!(view.disabled);
        assert!(
            view.notes
                .contains(&"settings.administrator.standardAccount")
        );
        // A failed read: no status, nothing to explain beyond the two lines.
        let failed = administrator_view(None, false, false, false);
        assert_eq!(
            (failed.state, failed.disabled, failed.notes.len()),
            ("settings.administrator.off", true, 2)
        );
    }
}
