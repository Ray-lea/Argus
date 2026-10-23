use super::*;

const SECTIONS: [&str; 9] = [
    "language",
    "startup",
    "hotkey",
    "privilege",
    "motion",
    "data",
    "rules",
    "update",
    "about",
];
impl Desktop {
    pub(in crate::frontend) fn settings_group(
        &self,
        title: &str,
        description: &str,
        controls: impl IntoElement,
        cx: &Context<Self>,
    ) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .border_1()
            .border_color(self.palette.stroke)
            .rounded(cx.theme().radius)
            .bg(self.palette.layer)
            .child(div().font_weight(FontWeight::SEMIBOLD).child(label(title)))
            .when(!description.is_empty(), |d| {
                d.child(
                    div()
                        .text_sm()
                        .text_color(self.palette.muted)
                        .child(label(description)),
                )
            })
            .child(controls)
            .into_any_element()
    }
    pub(in crate::frontend) fn setting_toggle(
        &self,
        id: &str,
        value: bool,
        command: &str,
        argument: &str,
        cx: &Context<Self>,
    ) -> AnyElement {
        self.button(
            id.to_owned(),
            label(if value {
                "settings.toggle.on"
            } else {
                "settings.toggle.off"
            }),
            None,
            Action::Invoke(command.into(), json!({argument:!value})),
            value,
            cx,
        )
        .into_any_element()
    }
    pub(in crate::frontend) fn setting_choices(
        &self,
        key: &str,
        selected: &str,
        options: &[(&str, String)],
        command: Option<&str>,
        cx: &Context<Self>,
    ) -> AnyElement {
        div()
            .flex()
            .flex_wrap()
            .gap_2()
            .children(options.iter().map(|(value, name)| {
                self.button(
                    format!("{key}-{value}"),
                    name.clone(),
                    None,
                    if let Some(command) = command {
                        Action::Invoke(command.into(), json!({key:value}))
                    } else {
                        Action::Preference(key.into(), json!(value))
                    },
                    selected == *value,
                    cx,
                )
            }))
            .into_any_element()
    }
    pub(in crate::frontend) fn settings_view(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let settings = self.data["settings"].clone();
        if !SECTIONS.contains(&self.settings_section.as_str()) {
            self.settings_section = SECTIONS[0].into();
        }
        let nav = div()
            .w_48()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap_1()
            .children(SECTIONS.into_iter().map(|section| {
                self.button(
                    format!("settings-{section}"),
                    label(&format!("settings.sections.{section}")),
                    None,
                    Action::Preference("settingsSection".into(), json!(section)),
                    self.settings_section == section,
                    cx,
                )
            }));
        let mut body = div()
            .id("settings-content")
            .flex_1()
            .min_w_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_4()
            .px_6()
            .pb_6();
        body = body.child(div().text_2xl().child(label(&format!(
            "settings.sections.{}",
            self.settings_section
        ))));
        match self.settings_section.as_str() {
            "language" => {
                let languages = [
                    ("system", label("common.languageSystem")),
                    ("zh-CN", label("common.languageName.zhCN")),
                    ("en-US", label("common.languageName.enUS")),
                ];
                body = body.child(self.settings_group(
                    "settings.language.title",
                    "settings.language.desc",
                    self.setting_choices(
                        "language",
                        text(&settings, "language"),
                        &languages,
                        Some("set_language"),
                        cx,
                    ),
                    cx,
                ));
                let themes = ["system", "light", "dark"]
                    .map(|value| (value, label(&format!("settings.theme.{value}"))));
                body = body.child(self.settings_group(
                    "settings.theme.title",
                    "settings.theme.desc",
                    self.setting_choices(
                        "theme",
                        text(&settings, "theme"),
                        &themes,
                        Some("set_theme"),
                        cx,
                    ),
                    cx,
                ));
                let positions = ["side", "top"]
                    .map(|value| (value, label(&format!("settings.navigation.{value}"))));
                body = body.child(self.settings_group(
                    "settings.navigation.title",
                    "settings.navigation.desc",
                    self.setting_choices(
                        "navPosition",
                        if self.nav_top { "top" } else { "side" },
                        &positions,
                        None,
                        cx,
                    ),
                    cx,
                ));
            }
            "startup" => {
                for (key, field, command, arg) in [
                    ("autostart", "autostart", "set_autostart", "enabled"),
                    ("closeToTray", "closeToTray", "set_close_to_tray", "value"),
                ] {
                    body = body.child(self.settings_group(
                        &format!("settings.startup.{key}.title"),
                        &format!("settings.startup.{key}.desc"),
                        self.setting_toggle(field, boolean(&settings, field), command, arg, cx),
                        cx,
                    ));
                }
            }
            "hotkey" => {
                body = body.child(
                    self.settings_group(
                        "settings.hotkey.title",
                        "settings.hotkey.desc",
                        div()
                            .flex()
                            .gap_2()
                            .child(self.setting_toggle(
                                "hotkeyEnabled",
                                boolean(&settings, "hotkeyEnabled"),
                                "set_hotkey_enabled",
                                "enabled",
                                cx,
                            ))
                            .child(self.button(
                                "hotkey-edit",
                                text(&settings, "hotkey").to_owned(),
                                None,
                                Action::Dialog("hotkey".into(), settings.clone()),
                                false,
                                cx,
                            ))
                            .child(self.button(
                                "hotkey-default",
                                label("settings.hotkey.reset"),
                                None,
                                Action::Invoke(
                                    "set_hotkey".into(),
                                    json!({"hotkey":crate::settings::DEFAULT_HOTKEY}),
                                ),
                                false,
                                cx,
                            )),
                        cx,
                    ),
                );
                body = body.child(self.settings_group(
                    "settings.shortcuts.title",
                    "settings.shortcuts.desc",
                    self.button(
                        "shortcuts",
                        label("settings.shortcuts.view"),
                        None,
                        Action::Dialog("shortcuts".into(), Value::Null),
                        false,
                        cx,
                    ),
                    cx,
                ));
            }
            "privilege" => {
                let privilege = self.data.get("privilege").cloned().unwrap_or(Value::Null);
                let mut controls = div().flex().flex_wrap().gap_2().child(self.button(
                    "privilege-refresh",
                    label("settings.refreshStatus"),
                    None,
                    Action::Invoke("privileged_service_status".into(), json!({})),
                    false,
                    cx,
                ));
                for (action, key) in [
                    ("install", "settings.privilege.install.action"),
                    ("start", "settings.privilege.start"),
                    ("update", "settings.privilege.update"),
                    ("uninstall", "settings.privilege.uninstall.action"),
                ] {
                    controls = controls.child(self.button(
                        format!("privilege-{action}"),
                        label(key),
                        None,
                        Action::Dialog("privilege".into(), json!({"action":action})),
                        false,
                        cx,
                    ));
                }
                body = body.child(
                    self.settings_group(
                        "common.privilegedService",
                        "settings.privilege.desc",
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(self.property("common.status", self.service_status(&privilege)))
                            .child(controls),
                        cx,
                    ),
                );
                body = body.child(
                    self.settings_group(
                        "settings.taskManager.title",
                        "settings.taskManager.desc",
                        div()
                            .flex()
                            .gap_2()
                            .child(self.button(
                                "taskmgr-enable",
                                label("settings.taskManager.enable"),
                                None,
                                Action::Dialog("taskmgr".into(), json!({"action":"enable"})),
                                false,
                                cx,
                            ))
                            .child(self.button(
                                "taskmgr-restore",
                                label("settings.taskManager.restore"),
                                None,
                                Action::Dialog("taskmgr".into(), json!({"action":"restore"})),
                                false,
                                cx,
                            )),
                        cx,
                    ),
                );
                body = body.child(self.settings_group(
                    "settings.sensors.title",
                    "settings.sensors.desc",
                    self.button(
                        "sensors",
                        label(if boolean(&settings, "kernelSensors") {
                            "common.on"
                        } else {
                            "common.off"
                        }),
                        None,
                        Action::Dialog(
                            "sensors".into(),
                            json!({"enabled":!boolean(&settings,"kernelSensors")}),
                        ),
                        boolean(&settings, "kernelSensors"),
                        cx,
                    ),
                    cx,
                ));
            }
            "motion" => {
                let options = [
                    ("system", label("common.languageSystem")),
                    ("on", label("common.on")),
                    ("off", label("common.off")),
                ];
                body = body.child(self.settings_group(
                    "settings.motion.ui.title",
                    "settings.motion.ui.desc",
                    self.setting_choices(
                        "uiMotion",
                        match self.prefs.get::<Option<bool>>("motion.ui", None) {
                            Some(true) => "on",
                            Some(false) => "off",
                            None => "system",
                        },
                        &options,
                        None,
                        cx,
                    ),
                    cx,
                ));
                let modes = [
                    (
                        "fluid",
                        tr!(
                            "settings.motion.chart.fluid",
                            fps = (1.
                                / motion::frame_period(
                                    motion::Mode::Fluid,
                                    window.refresh_interval(cx)
                                )
                                .as_secs_f64())
                            .round() as u32
                        ),
                    ),
                    (
                        "balanced",
                        tr!(
                            "settings.motion.chart.balanced",
                            fps = (1.
                                / motion::frame_period(
                                    motion::Mode::Balanced,
                                    window.refresh_interval(cx)
                                )
                                .as_secs_f64())
                            .round() as u32
                        ),
                    ),
                    ("pixel", label("settings.motion.chart.pixel")),
                    ("off", label("settings.motion.chart.off")),
                ];
                body = body.child(
                    self.settings_group(
                        "settings.motion.chart.title",
                        "",
                        self.setting_choices(
                            "motion.chartMode",
                            &self
                                .prefs
                                .get::<Option<String>>("motion.chartMode", None)
                                .unwrap_or_else(|| {
                                    if crate::platform::system_motion() {
                                        "fluid"
                                    } else {
                                        "off"
                                    }
                                    .into()
                                }),
                            &modes,
                            None,
                            cx,
                        ),
                        cx,
                    ),
                );
            }
            "data" => {
                for (field, title) in [("minimizedInterval", "minimized"), ("trayInterval", "tray")]
                {
                    let mut controls = div().flex().gap_2().flex_wrap();
                    for ms in crate::settings::BACKGROUND_INTERVALS {
                        let mut args = json!({"minimized":settings["minimizedInterval"],"tray":settings["trayInterval"]});
                        args[title] = json!(ms);
                        controls = controls.child(self.button(
                            format!("{field}-{ms}"),
                            tr!("shell.interval.option", seconds = ms as f64 / 1000.),
                            None,
                            Action::Invoke("set_background_intervals".into(), args),
                            settings[field].as_u64() == Some(ms),
                            cx,
                        ));
                    }
                    body = body.child(self.settings_group(
                        &format!("settings.data.{title}.title"),
                        &format!("settings.data.{title}.desc"),
                        controls,
                        cx,
                    ));
                }
                let units = [
                    ("bytes", label("settings.data.netUnit.bytes")),
                    ("bits", label("settings.data.netUnit.bits")),
                ];
                body = body.child(self.settings_group(
                    "settings.data.netUnit.title",
                    "settings.data.netUnit.desc",
                    self.setting_choices(
                        "netUnit",
                        &self.prefs.get::<String>("netUnit", "bytes".into()),
                        &units,
                        None,
                        cx,
                    ),
                    cx,
                ));
                body = body.child(
                    self.settings_group(
                        "settings.data.history.title",
                        if boolean(&settings, "history") {
                            "settings.data.history.on"
                        } else {
                            "settings.data.history.off"
                        },
                        div()
                            .flex()
                            .gap_2()
                            .child(self.setting_toggle(
                                "history",
                                boolean(&settings, "history"),
                                "set_history_enabled",
                                "enabled",
                                cx,
                            ))
                            .child(self.button(
                                "clear-history",
                                label("settings.data.history.clear"),
                                None,
                                Action::Dialog("clearHistory".into(), Value::Null),
                                false,
                                cx,
                            )),
                        cx,
                    ),
                );
            }
            "rules" => {
                body = body
                    .child(
                        div()
                            .text_sm()
                            .text_color(self.palette.muted)
                            .child(label("settings.rules.desc")),
                    )
                    .child(self.button(
                        "add-rule",
                        label("settings.rules.add"),
                        None,
                        Action::Dialog("rule".into(), Value::Null),
                        false,
                        cx,
                    ));
                for rule in values(&settings["rules"]) {
                    let id = text(rule, "id");
                    body = body.child(
                        self.settings_group(
                            text(rule, "name"),
                            "",
                            div()
                                .flex()
                                .gap_2()
                                .child(self.button(
                                    format!("rule-toggle-{id}"),
                                    label(if boolean(rule, "enabled") {
                                        "settings.rules.disable"
                                    } else {
                                        "settings.rules.enable"
                                    }),
                                    None,
                                    Action::Preference("ruleToggle".into(), json!(id)),
                                    false,
                                    cx,
                                ))
                                .child(self.button(
                                    format!("rule-edit-{id}"),
                                    label("processes.menu.setPriority"),
                                    None,
                                    Action::Dialog("rule".into(), rule.clone()),
                                    false,
                                    cx,
                                ))
                                .child(self.button(
                                    format!("rule-delete-{id}"),
                                    label("settings.rules.delete"),
                                    None,
                                    Action::Preference("ruleDelete".into(), json!(id)),
                                    false,
                                    cx,
                                )),
                            cx,
                        ),
                    );
                }
            }
            "update" => {
                let status = self.data.get("updates").cloned().unwrap_or(Value::Null);
                body = body
                    .child(tr!(
                        "settings.update.current",
                        version = text(&self.data["info"], "version"),
                        channel = label(&format!(
                            "settings.update.channel.{}",
                            text(&settings, "updateChannel")
                        ))
                    ))
                    .child(self.property(
                        "common.status",
                        argus_core::i18n::format(
                            &format!(
                                "settings.update.status.{}",
                                status["state"].as_str().unwrap_or("loading")
                            ),
                            &[("version", &text(&status, "version"))],
                        ),
                    ));
                body = body.child(
                    div()
                        .flex()
                        .gap_2()
                        .child(self.button(
                            "check-update",
                            label("settings.update.check"),
                            None,
                            Action::Invoke("check_update".into(), json!({})),
                            false,
                            cx,
                        ))
                        .child(
                            self.button(
                                "install-update",
                                label("settings.update.restart"),
                                None,
                                Action::Dialog("update".into(), status.clone()),
                                false,
                                cx,
                            )
                            .disabled(!matches!(text(&status, "state"), "ready" | "available")),
                        ),
                );
                body = body.child(self.settings_group(
                    "settings.update.auto.title",
                    "settings.update.auto.desc",
                    self.button(
                        "update-auto",
                        label(if boolean(&settings, "updateCheck") {
                            "common.on"
                        } else {
                            "common.off"
                        }),
                        None,
                        Action::Preference(
                            "updateCheck".into(),
                            json!(!boolean(&settings, "updateCheck")),
                        ),
                        boolean(&settings, "updateCheck"),
                        cx,
                    ),
                    cx,
                ));
                for (field, namespace, options) in [
                    ("updateChannel", "channel", vec!["stable", "beta"]),
                    ("updateRoute", "route", vec!["auto", "direct", "proxy"]),
                ] {
                    let choices = options
                        .iter()
                        .map(|v| (*v, label(&format!("settings.update.{namespace}.{v}"))))
                        .collect::<Vec<_>>();
                    body = body.child(self.settings_group(
                        &format!("settings.update.{namespace}.label"),
                        &format!("settings.update.{namespace}.desc"),
                        self.setting_choices(field, text(&settings, field), &choices, None, cx),
                        cx,
                    ));
                }
                body = body.child(self.settings_group(
                    "settings.update.proxy.label",
                    "settings.update.proxy.fallback",
                    self.button(
                        "proxy-edit",
                        text(&settings, "updateProxy").to_owned(),
                        None,
                        Action::Dialog("proxy".into(), settings.clone()),
                        false,
                        cx,
                    ),
                    cx,
                ));
                body = body.child(self.settings_group(
                    "settings.update.whatsNew.title",
                    "settings.update.whatsNew.desc",
                    self.setting_toggle(
                        "whatsNew",
                        boolean(&settings, "whatsNew"),
                        "set_whats_new",
                        "enabled",
                        cx,
                    ),
                    cx,
                ));
            }
            "about" => {
                body = body
                    .child(div().text_3xl().child(argus_app::config::product_name()))
                    .child(text(&self.data["info"], "version").to_owned())
                    .child(label("settings.about.tagline"));
                for (id, key, command) in [
                    ("homepage", "settings.about.homepage.open", "open_homepage"),
                    (
                        "community",
                        "settings.about.community.join",
                        "open_community",
                    ),
                    ("log", "settings.about.log.open", "open_log_dir"),
                ] {
                    body = body.child(self.button(
                        id,
                        label(key),
                        None,
                        Action::Invoke(command.into(), json!({})),
                        false,
                        cx,
                    ));
                }
                for (id, key) in [
                    ("whatsnew", "settings.about.whatsNew"),
                    ("license", "settings.about.license.view"),
                    ("notices", "settings.about.notices.view"),
                    ("onboarding", "settings.about.onboarding.open"),
                ] {
                    body = body.child(self.button(
                        id,
                        label(key),
                        None,
                        Action::Dialog(id.into(), Value::Null),
                        false,
                        cx,
                    ));
                }
                body = body.child(self.button(
                    "quit",
                    label("settings.about.quit"),
                    None,
                    Action::Invoke("quit".into(), json!({})),
                    false,
                    cx,
                ));
            }
            _ => {}
        }
        div()
            .flex_1()
            .min_h_0()
            .px_3()
            .flex()
            .child(nav)
            .child(body)
            .into_any_element()
    }
    pub(in crate::frontend) fn service_status(&self, status: &Value) -> String {
        if text(status, "state") == "running"
            && !boolean(status, "incompatible")
            && status["version"].is_string()
        {
            tr!(
                "settings.privilege.state.connected",
                version = text(status, "version")
            )
        } else {
            let state = if boolean(status, "incompatible") {
                "incompatible"
            } else if text(status, "state") == "running" {
                "unreachable"
            } else {
                text(status, "state")
            };
            let key = format!("settings.privilege.state.{state}");
            let result = label(&key);
            if result == key {
                label("settings.privilege.state.unknown")
            } else {
                result
            }
        }
    }
}
