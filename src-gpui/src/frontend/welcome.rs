//! Native onboarding and release notes share the product metadata and catalogs.
use super::*;
use gpui_kit::component::dialog::Dialog;
pub(super) fn configuration() -> &'static Value {
    static CONFIG: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    CONFIG.get_or_init(|| {
        serde_json::from_str(include_str!("../../../resources/desktop.json"))
            .expect("desktop configuration")
    })
}
pub(super) fn current_notes() -> String {
    let source = match argus_core::i18n::locale() {
        argus_core::i18n::Locale::ZhCn => include_str!("../../../CHANGELOG.md"),
        argus_core::i18n::Locale::EnUs => include_str!("../../../CHANGELOG.en.md"),
    };
    let heading = format!("## [{}]", env!("CARGO_PKG_VERSION"));
    let mut lines = source
        .lines()
        .skip_while(|line| !line.starts_with(&heading));
    let _ = lines.next();
    lines
        .take_while(|line| !line.starts_with("## ["))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .into()
}
impl Desktop {
    pub(in crate::frontend) fn check_welcome(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.welcome_checked
            || self.options.test
            || self.snapshot.is_null()
            || self.modal.is_some()
        {
            return;
        }
        if !self
            .host
            .get_window("main")
            .is_some_and(|w| w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false))
        {
            return;
        }
        self.welcome_checked = true;
        if boolean(&self.data["info"], "taskManager") {
            return;
        }
        if number(&self.data["settings"], "onboarding")
            < number(configuration(), "onboardingVersion")
        {
            self.open_dialog("onboarding", json!({"step":0}), window, cx);
            return;
        }
        if boolean(&self.data["info"], "updated")
            && text(&self.data["settings"], "seenVersion") != env!("CARGO_PKG_VERSION")
        {
            if boolean(&self.data["settings"], "whatsNew") {
                self.open_dialog("whatsnew", Value::Null, window, cx);
            } else {
                self.toast = Some((
                    tr!("updates.updated", version = env!("CARGO_PKG_VERSION")),
                    Instant::now(),
                ));
            }
            self.request("note_version_seen", json!({}), Some("settings"), window, cx);
        }
    }
    pub(in crate::frontend) fn onboarding_dialog(
        &self,
        dialog: Dialog,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Dialog {
        let step = self
            .modal
            .as_ref()
            .and_then(|m| m.args["step"].as_u64())
            .unwrap_or(0) as usize;
        let steps = values(&configuration()["onboardingSteps"]);
        let section = steps.get(step).and_then(Value::as_str).unwrap_or("tips");
        let settings = &self.data["settings"];
        let mut body = div()
            .flex()
            .flex_col()
            .gap_4()
            .child(tr!(
                "onboarding.step",
                current = step + 1,
                total = steps.len()
            ))
            .child(label(&format!("onboarding.{section}.body")));
        match section {
            "welcome" => {
                let choices = [
                    ("system", label("common.languageSystem")),
                    ("zh-CN", label("common.languageName.zhCN")),
                    ("en-US", label("common.languageName.enUS")),
                ];
                body = body.child(self.setting_choices(
                    "language",
                    text(settings, "language"),
                    &choices,
                    Some("set_language"),
                    cx,
                ));
            }
            "display" => {
                let choices = [
                    ("side", label("settings.navigation.side")),
                    ("top", label("settings.navigation.top")),
                ];
                body = body.child(self.setting_choices(
                    "navPosition",
                    if self.nav_top { "top" } else { "side" },
                    &choices,
                    None,
                    cx,
                ));
                let mut intervals = div().flex().gap_2();
                for interval in crate::settings::BACKGROUND_INTERVALS
                    .into_iter()
                    .filter(|ms| *ms <= 5000)
                {
                    intervals = intervals.child(self.button(
                        format!("welcome-interval-{interval}"),
                        tr!("shell.interval.option", seconds = interval as f64 / 1000.),
                        None,
                        Action::Interval(interval),
                        interval == self.interval,
                        cx,
                    ));
                }
                body = body.child(intervals);
            }
            "background" => {
                body = body
                    .child(label("onboarding.background.autostart"))
                    .child(self.setting_toggle(
                        "welcome-autostart",
                        boolean(settings, "autostart"),
                        "set_autostart",
                        "enabled",
                        cx,
                    ))
                    .child(label("onboarding.background.closeToTray"))
                    .child(self.setting_toggle(
                        "welcome-tray",
                        boolean(settings, "closeToTray"),
                        "set_close_to_tray",
                        "value",
                        cx,
                    ));
            }
            "update" => {
                body = body.child(self.button(
                    "welcome-update",
                    label("onboarding.update.check"),
                    None,
                    Action::Preference(
                        "updateCheck".into(),
                        json!(!boolean(settings, "updateCheck")),
                    ),
                    boolean(settings, "updateCheck"),
                    cx,
                ));
            }
            "privilege" | "sensors" | "taskManager" => {
                if section == "privilege" {
                    body = body.child(label("onboarding.privilege.risk"));
                }
                body = body
                    .child(label(&format!("onboarding.{section}.later")))
                    .child(self.button(
                        "welcome-privilege",
                        label("onboarding.sensors.configureService"),
                        None,
                        Action::Preference("configurePrivilege".into(), Value::Null),
                        false,
                        cx,
                    ));
            }
            "tips" => {
                for key in ["menu", "drop", "history", "reopen"] {
                    body = body.child(label(&format!("onboarding.tips.{key}")));
                }
            }
            _ => {}
        }
        let owner = cx.entity().downgrade();
        dialog
            .title(label(&format!("onboarding.{section}.title")))
            .overlay_closable(false)
            .child(body)
            .footer(
                div()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .when(step > 0, |d| {
                        d.child(self.button(
                            "welcome-back",
                            label("onboarding.back"),
                            None,
                            Action::Preference("dialog.step".into(), json!(step - 1)),
                            false,
                            cx,
                        ))
                    })
                    .child(self.button(
                        "welcome-skip",
                        label("onboarding.skip"),
                        None,
                        Action::Preference("finishOnboarding".into(), Value::Null),
                        false,
                        cx,
                    ))
                    .child(self.button(
                        "welcome-next",
                        label(if step + 1 < steps.len() {
                            "onboarding.next"
                        } else {
                            "onboarding.finish"
                        }),
                        None,
                        if step + 1 < steps.len() {
                            Action::Preference("dialog.step".into(), json!(step + 1))
                        } else {
                            Action::Preference("finishOnboarding".into(), Value::Null)
                        },
                        true,
                        cx,
                    )),
            )
            .on_close(move |_, window, cx| {
                let _ = owner.update(cx, |this, cx| {
                    this.preference("finishOnboarding", Value::Null, window, cx)
                });
            })
    }
}

pub(super) fn update_notes(status: &Value) -> String {
    let locale = match argus_core::i18n::locale() {
        argus_core::i18n::Locale::EnUs => "en-US",
        _ => "zh-CN",
    };
    let current = semver::Version::parse(env!("CARGO_PKG_VERSION")).unwrap();
    let notes = values(&status["releases"])
        .iter()
        .filter(|release| {
            semver::Version::parse(text(release, "version")).is_ok_and(|v| v > current)
        })
        .map(|release| {
            let body = release["translations"][locale]
                .as_str()
                .unwrap_or_else(|| text(release, "notes"));
            format!("## {}\n\n{}", text(release, "version"), body)
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    if notes.is_empty() {
        text(status, "notes").to_owned()
    } else {
        notes
    }
}
