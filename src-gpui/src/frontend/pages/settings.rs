use super::*;
use gpui_kit::component::scroll::{ScrollableElement, ScrollbarAxis};

pub(super) const SECTIONS: [&str; 9] = [
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
fn section_icon(section: &str) -> &'static str {
    match section {
        "language" => "LanguageIcon",
        "startup" => "PowerIcon",
        "hotkey" => "KeyboardIcon",
        "privilege" => "ShieldIcon",
        "motion" => "MotionIcon",
        "data" => "DataIcon",
        "rules" => "RulesIcon",
        "update" => "DownloadIcon",
        _ => "InfoIcon",
    }
}
fn setting_icon(title: &str) -> &'static str {
    if title.contains("theme") {
        "ThemeIcon"
    } else if title.contains("navigation") {
        "PanelIcon"
    } else {
        section_icon(title.split('.').nth(1).unwrap_or_default())
    }
}
impl Desktop {
    pub(in crate::frontend) fn settings_group(
        &self,
        title: &str,
        description: &str,
        controls: impl IntoElement,
        cx: &Context<Self>,
    ) -> AnyElement {
        self.settings_group_text(title, label(description), controls, cx)
    }
    fn settings_group_text(
        &self,
        title: &str,
        description: String,
        controls: impl IntoElement,
        cx: &Context<Self>,
    ) -> AnyElement {
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_5()
            .p_4()
            .border_1()
            .border_color(self.palette.stroke)
            .rounded(cx.theme().radius)
            .bg(self.palette.layer)
            .child(icon(setting_icon(title), self.palette.muted).size_5())
            .child(
                div()
                    .flex_1()
                    .min_w_48()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(div().font_weight(FontWeight::MEDIUM).child(label(title)))
                    .when(!description.is_empty(), |d| {
                        d.child(
                            div()
                                .text_xs()
                                .text_color(self.palette.muted)
                                .child(description),
                        )
                    }),
            )
            .child(div().flex_shrink_0().max_w_96().child(controls))
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
        self.switch(
            id.to_owned(),
            id.to_owned(),
            value,
            Action::Invoke(command.into(), json!({argument:!value})),
            cx,
        )
    }
    pub(in crate::frontend) fn setting_choices(
        &self,
        key: &str,
        selected: &str,
        options: &[(&str, String)],
        command: Option<&str>,
        cx: &Context<Self>,
    ) -> AnyElement {
        self.segments(
            format!("setting-{key}"),
            options
                .iter()
                .map(|(value, name)| {
                    (
                        name.clone(),
                        None,
                        if let Some(command) = command {
                            Action::Invoke(command.into(), json!({key:value}))
                        } else {
                            Action::Preference(key.into(), json!(value))
                        },
                        selected == *value,
                    )
                })
                .collect(),
            cx,
        )
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
            .w(theme::dimension("settingsRailWidth"))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap_1()
            .children(SECTIONS.into_iter().map(|section| {
                self.button(
                    format!("settings-{section}"),
                    label(&format!("settings.sections.{section}")),
                    Some(section_icon(section)),
                    Action::Preference("settingsSection".into(), json!(section)),
                    self.settings_section == section,
                    cx,
                )
                .justify_start()
                .relative()
                .h_10()
                .when(self.settings_section == section, |b| {
                    b.child(
                        div()
                            .absolute()
                            .left_0()
                            .w_1()
                            .h_4()
                            .rounded_full()
                            .bg(self.palette.accent),
                    )
                })
            }));
        let sections = SECTIONS
            .into_iter()
            .map(|section| self.settings_section_view(section, &settings, window, cx))
            .collect::<Vec<_>>();
        let body = div()
            .id("settings-content")
            .flex_1()
            .min_w_0()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.settings_scroll)
            .flex()
            .flex_col()
            .gap_7()
            .pl_6()
            .pr_2()
            .pb_8()
            .children(sections)
            .on_scroll_wheel(cx.listener(|this, _, window, cx| {
                let owner = cx.entity().downgrade();
                window.on_next_frame(move |_, cx| {
                    let _ = owner.update(cx, |this, cx| {
                        let index = this.settings_scroll.top_item().min(SECTIONS.len() - 1);
                        if this.settings_section != SECTIONS[index] {
                            this.settings_section = SECTIONS[index].into();
                            cx.notify();
                        }
                    });
                });
                let _ = this;
            }));
        let body = body.scrollbar(&self.settings_scroll, ScrollbarAxis::Vertical);
        if let Some(index) = self.settings_target.take() {
            let owner = cx.entity().downgrade();
            window.on_next_frame(move |_, cx| {
                let _ = owner.update(cx, |this, cx| {
                    this.settings_scroll.scroll_to_top_of_item(index);
                    cx.notify();
                });
            });
        }
        div()
            .size_full()
            .min_h_0()
            .px_3()
            .py_2()
            .flex()
            .justify_center()
            .child(
                div()
                    .flex()
                    .w_full()
                    .max_w(theme::dimension("settingsMaxWidth"))
                    .min_h_0()
                    .child(nav)
                    .child(body),
            )
            .into_any_element()
    }
    fn settings_section_view(
        &mut self,
        section: &str,
        settings: &Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut body = div()
            .id(format!("section-{section}"))
            .flex()
            .flex_col()
            .flex_shrink_0()
            .gap_1()
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .py_2()
                    .child(label(&format!("settings.sections.{section}"))),
            );
        match section {
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
                        text(settings, "language"),
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
                        text(settings, "theme"),
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
                        self.setting_toggle(field, boolean(settings, field), command, arg, cx),
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
                                boolean(settings, "hotkeyEnabled"),
                                "set_hotkey_enabled",
                                "enabled",
                                cx,
                            ))
                            .child(self.button(
                                "hotkey-edit",
                                text(settings, "hotkey").to_owned(),
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
                let taskmgr = self.data.get("taskmgr").cloned().unwrap_or(Value::Null);
                let tm_state = taskmgr["state"].as_str().unwrap_or("disabled");
                let tm_status_label = {
                    let k = format!("settings.taskManager.state.{tm_state}");
                    let l = label(&k);
                    if l == k {
                        label("settings.taskManager.unknown")
                    } else {
                        l
                    }
                };
                body = body.child(
                    self.settings_group(
                        "settings.taskManager.title",
                        "settings.taskManager.desc",
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(self.property("common.status", tm_status_label))
                            .child(
                                div()
                                    .flex()
                                    .gap_2()
                                    .child(self.button(
                                        "taskmgr-enable",
                                        label("settings.taskManager.enable"),
                                        None,
                                        Action::Dialog(
                                            "taskmgr".into(),
                                            json!({"action":"enable"}),
                                        ),
                                        false,
                                        cx,
                                    ))
                                    .child(self.button(
                                        "taskmgr-restore",
                                        label("settings.taskManager.restore"),
                                        None,
                                        Action::Dialog(
                                            "taskmgr".into(),
                                            json!({"action":"restore"}),
                                        ),
                                        false,
                                        cx,
                                    )),
                            ),
                        cx,
                    ),
                );
                body = body.child(self.settings_group(
                    "settings.sensors.title",
                    "settings.sensors.desc",
                    self.button(
                        "sensors",
                        label(if boolean(settings, "kernelSensors") {
                            "common.on"
                        } else {
                            "common.off"
                        }),
                        None,
                        Action::Dialog(
                            "sensors".into(),
                            json!({"enabled":!boolean(settings,"kernelSensors")}),
                        ),
                        boolean(settings, "kernelSensors"),
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
                let usage = self
                    .data
                    .get("historyUsage")
                    .cloned()
                    .unwrap_or(Value::Null);
                let history_size_note = if usage.is_null() {
                    label("settings.data.history.counting")
                } else if number(&usage, "bytes") <= 0. {
                    label("settings.data.history.empty")
                } else if let Some(oldest) = usage["oldestTs"].as_i64().filter(|t| *t > 0) {
                    tr!(
                        "settings.data.history.sizeSince",
                        size = theme::bytes(number(&usage, "bytes"), false),
                        date = theme::timestamp(oldest)
                    )
                } else {
                    tr!(
                        "settings.data.history.size",
                        size = theme::bytes(number(&usage, "bytes"), false)
                    )
                };
                let history_desc = format!(
                    "{} {}",
                    label(if boolean(settings, "history") {
                        "settings.data.history.on"
                    } else {
                        "settings.data.history.off"
                    }),
                    history_size_note
                );
                body = body.child(
                    self.settings_group_text(
                        "settings.data.history.title",
                        history_desc,
                        div()
                            .flex()
                            .gap_2()
                            .child(self.setting_toggle(
                                "history",
                                boolean(settings, "history"),
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
                let rule_stats = self.data.get("ruleStats").cloned().unwrap_or(Value::Null);
                for rule in values(&settings["rules"]) {
                    let id = text(rule, "id");
                    let enabled = boolean(rule, "enabled");
                    let priority_str = rule["priority"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .map(|p| {
                            tr!(
                                "settings.rules.priority.option",
                                priority = label(&format!("processes.priority.{p}"))
                            )
                        })
                        .unwrap_or_else(|| label("settings.rules.priority.keep"));
                    let eff_str = match rule["efficiency"].as_bool() {
                        Some(true) => label("settings.rules.efficiency.on"),
                        Some(false) => label("settings.rules.efficiency.off"),
                        None => label("settings.rules.efficiency.keep"),
                    };
                    let stat_summary = if !enabled {
                        label("settings.rules.summary.disabled")
                    } else if rule["priority"].is_null() && rule["efficiency"].is_null() {
                        label("settings.rules.summary.noAction")
                    } else if let Some(st) = values(&rule_stats)
                        .iter()
                        .find(|s| text(s, "id") == id || text(s, "name") == text(rule, "name"))
                    {
                        let applied = st["applied"].as_u64().unwrap_or(0);
                        let denied = st["denied"].as_u64().unwrap_or(0);
                        let failed = st["failed"].as_u64().unwrap_or(0);
                        let mut parts = Vec::new();
                        if applied > 0 {
                            parts.push(tr!("settings.rules.summary.applied", count = applied));
                        }
                        if denied > 0 {
                            parts.push(tr!("settings.rules.summary.denied", count = denied));
                        }
                        if failed > 0 {
                            parts.push(tr!("settings.rules.summary.failed", count = failed));
                        }
                        if parts.is_empty() {
                            label("settings.rules.summary.notSeen")
                        } else {
                            tr!(
                                "settings.rules.summary.sentence",
                                parts = parts.join(&label("settings.rules.summary.separator"))
                            )
                        }
                    } else {
                        label("settings.rules.summary.notSeen")
                    };
                    let desc = format!("{priority_str} · {eff_str}\n{stat_summary}");
                    body = body.child(
                        self.settings_group_text(
                            text(rule, "name"),
                            desc,
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(self.switch(
                                    format!("rule-toggle-{id}"),
                                    tr!("settings.rules.toggle", name = text(rule, "name")),
                                    enabled,
                                    Action::Preference("ruleToggle".into(), json!(id)),
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
                            text(settings, "updateChannel")
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
                    self.switch(
                        "update-auto",
                        label("settings.update.auto.title"),
                        boolean(settings, "updateCheck"),
                        Action::Preference(
                            "updateCheck".into(),
                            json!(!boolean(settings, "updateCheck")),
                        ),
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
                        self.setting_choices(field, text(settings, field), &choices, None, cx),
                        cx,
                    ));
                }
                body = body.child(self.settings_group(
                    "settings.update.proxy.label",
                    "settings.update.proxy.fallback",
                    self.button(
                        "proxy-edit",
                        text(settings, "updateProxy").to_owned(),
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
                        boolean(settings, "whatsNew"),
                        "set_whats_new",
                        "enabled",
                        cx,
                    ),
                    cx,
                ));
            }
            "about" => {
                let manifest = argus_app::config::manifest();
                let privilege = self.data.get("privilege").unwrap_or(&Value::Null);
                body = body
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_5()
                            .py_3()
                            .child(img("argus/logo.svg").size_16())
                            .child(
                                div()
                                    .flex_1()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(
                                        div()
                                            .flex()
                                            .items_baseline()
                                            .gap_3()
                                            .child(
                                                div()
                                                    .text_3xl()
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .child(argus_app::config::product_name()),
                                            )
                                            .child(div().text_color(self.palette.muted).child(
                                                text(&self.data["info"], "version").to_owned(),
                                            )),
                                    )
                                    .child(label("settings.about.tagline"))
                                    .child(div().text_xs().text_color(self.palette.muted).child(
                                        label(if boolean(&self.data["info"], "elevated") {
                                            "settings.about.admin"
                                        } else if text(privilege, "state") == "running" {
                                            "settings.about.service"
                                        } else {
                                            "settings.about.standard"
                                        }),
                                    )),
                            )
                            .child(self.button(
                                "whatsnew",
                                label("settings.about.whatsNew"),
                                None,
                                Action::Dialog("whatsnew".into(), Value::Null),
                                false,
                                cx,
                            )),
                    )
                    .child(self.settings_group_text(
                        "settings.about.homepage.title",
                        tr!(
                            "settings.about.homepage.desc",
                            url = text(manifest, "homepage")
                        ),
                        self.button(
                            "homepage",
                            label("settings.about.homepage.open"),
                            Some("ExternalLinkIcon"),
                            Action::Invoke("open_homepage".into(), json!({})),
                            false,
                            cx,
                        ),
                        cx,
                    ))
                    .child(
                        self.settings_group_text(
                            "settings.about.community.title",
                            tr!(
                                "settings.about.community.desc",
                                group = text(&manifest["community"], "group")
                            ),
                            div()
                                .flex()
                                .gap_2()
                                .child(self.button(
                                    "copy-community",
                                    label("settings.about.community.copy"),
                                    Some("CopyIcon"),
                                    Action::Copy(text(&manifest["community"], "group").into()),
                                    false,
                                    cx,
                                ))
                                .child(self.button(
                                    "community",
                                    label("settings.about.community.join"),
                                    Some("ExternalLinkIcon"),
                                    Action::Invoke("open_community".into(), json!({})),
                                    false,
                                    cx,
                                )),
                            cx,
                        ),
                    );
                for (id, key) in [
                    ("license", "settings.about.license.view"),
                    ("notices", "settings.about.notices.view"),
                    ("onboarding", "settings.about.onboarding.open"),
                ] {
                    body = body.child(self.settings_group(
                        &format!("settings.about.{id}.title"),
                        &format!("settings.about.{id}.desc"),
                        self.button(
                            id,
                            label(key),
                            None,
                            Action::Dialog(id.into(), Value::Null),
                            false,
                            cx,
                        ),
                        cx,
                    ));
                }
                body = body
                    .child(self.settings_group(
                        "settings.about.log.title",
                        "settings.about.log.desc",
                        self.button(
                            "log",
                            label("settings.about.log.open"),
                            Some("FolderIcon"),
                            Action::Invoke("open_log_dir".into(), json!({})),
                            false,
                            cx,
                        ),
                        cx,
                    ))
                    .child(div().flex().justify_end().child(self.button(
                        "quit",
                        label("settings.about.quit"),
                        None,
                        Action::Invoke("quit".into(), json!({})),
                        false,
                        cx,
                    )));
            }
            _ => {}
        }
        body.into_any_element()
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
