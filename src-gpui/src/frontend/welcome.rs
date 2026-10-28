//! Native onboarding, release notes and the update dialog (Onboarding.tsx,
//! UpdateDialog.tsx and ReleaseNotes.tsx in the original).
use super::*;
use gpui_kit::component::dialog::Dialog;
use widgets::{ButtonExt, Handler, ToastKind, Tone};

pub(super) fn configuration() -> &'static Value {
    static CONFIG: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    CONFIG.get_or_init(|| {
        serde_json::from_str(include_str!("../../../resources/desktop.json"))
            .expect("desktop configuration")
    })
}

/// One release of CHANGELOG.md: version, date and its notes.
#[derive(Clone)]
pub(super) struct Release {
    pub version: String,
    pub date: String,
    pub notes: String,
}

/// The releases written into the app's changelog, newest first.
pub(super) fn changelog() -> Vec<Release> {
    let source = match argus_core::i18n::locale() {
        argus_core::i18n::Locale::ZhCn => include_str!("../../../CHANGELOG.md"),
        argus_core::i18n::Locale::EnUs => include_str!("../../../CHANGELOG.en.md"),
    };
    let mut releases: Vec<Release> = vec![];
    for line in source.lines() {
        if let Some(rest) = line.strip_prefix("## [") {
            let version = rest.split(']').next().unwrap_or_default().to_owned();
            let date = rest
                .rsplit(" - ")
                .next()
                .filter(|d| d.len() == 10)
                .unwrap_or_default()
                .to_owned();
            releases.push(Release {
                version,
                date,
                notes: String::new(),
            });
        } else if let Some(release) = releases.last_mut() {
            release.notes.push_str(line);
            release.notes.push('\n');
        }
    }
    for release in &mut releases {
        release.notes = release.notes.trim().to_owned();
    }
    releases
}

fn version(text: &str) -> Option<semver::Version> {
    semver::Version::parse(text.trim_start_matches('v')).ok()
}

/// Releases after `from` up to and including `to`, newest first.
fn between(releases: Vec<Release>, from: Option<&str>, to: &str) -> Vec<Release> {
    let to = version(to);
    let from = from.and_then(version);
    let mut list = releases
        .into_iter()
        .filter(|r| {
            let Some(v) = version(&r.version) else {
                return false;
            };
            to.as_ref().is_none_or(|to| v <= *to) && from.as_ref().is_none_or(|from| v > *from)
        })
        .collect::<Vec<_>>();
    if from.is_none() {
        list.truncate(1);
    }
    list
}

/// The releases the update status carries, in the reader's language.
fn offered_releases(status: &Value) -> Vec<Release> {
    let locale = match argus_core::i18n::locale() {
        argus_core::i18n::Locale::EnUs => "en-US",
        _ => "zh-CN",
    };
    values(&status["releases"])
        .iter()
        .map(|release| Release {
            version: text(release, "version").to_owned(),
            date: text(release, "date").to_owned(),
            notes: release["translations"][locale]
                .as_str()
                .unwrap_or_else(|| text(release, "notes"))
                .to_owned(),
        })
        .collect()
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
            let from = self.prefs.get::<Option<String>>("update.from", None);
            self.prefs.set("update.from", Value::Null);
            if boolean(&self.data["settings"], "whatsNew") {
                self.open_dialog("whatsnew", json!({"from":from}), window, cx);
            } else {
                let args = json!({"from":from});
                self.toast_with(
                    ToastKind::Success,
                    tr!("updates.updated", version = env!("CARGO_PKG_VERSION")),
                    10000,
                    vec![(
                        label("updates.whatsNew"),
                        Rc::new(
                            move |this: &mut Desktop,
                                  window: &mut Window,
                                  cx: &mut Context<Desktop>| {
                                this.open_dialog("whatsnew", args.clone(), window, cx)
                            },
                        ),
                    )],
                );
            }
            self.request("note_version_seen", json!({}), Some("settings"), window, cx);
            self.ui.errors.insert("offerService".into(), "1".into());
            self.request(
                "privileged_service_status",
                json!({}),
                Some("privilege"),
                window,
                cx,
            );
        }
    }

    /// Says once per version that an update waits, with a way to its dialog;
    /// after an update, offers to update an older privileged service.
    pub(in crate::frontend) fn announce_update(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let onboarding = self
            .modal
            .iter()
            .chain(&self.ui.below)
            .any(|m| m.kind == "onboarding");
        let visible = self
            .host
            .get_window("main")
            .is_some_and(|w| w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false));
        if onboarding || !visible || self.options.test {
            return;
        }
        if self.modal.is_none()
            && self.ui.errors.contains_key("offerService")
            && let Some(service) = self.data.get("privilege").cloned()
        {
            self.ui.errors.remove("offerService");
            let app = env!("CARGO_PKG_VERSION");
            if text(&service, "state") == "running"
                && boolean(&service, "authorized")
                && text(&service, "version") != app
            {
                let body = match service["version"].as_str() {
                    Some(old) => tr!("updates.service.body", serviceVersion = old, version = app),
                    None => tr!("updates.service.bodyOlder", version = app),
                };
                self.ask(
                    label("updates.service.title"),
                    body,
                    label("updates.service.confirm"),
                    false,
                    |this, window, cx| this.service_action("update".into(), window, cx),
                    window,
                    cx,
                );
                return;
            }
        }
        let Some(status) = self.data.get("updates") else {
            return;
        };
        let state = text(status, "state");
        let version = text(status, "version").to_owned();
        if version.is_empty() || !matches!(state, "ready" | "available") {
            return;
        }
        let announced = self.prefs.get::<Option<String>>("update.announced", None);
        if announced.as_deref() == Some(version.as_str()) {
            return;
        }
        self.prefs.set("update.announced", &version);
        if self.modal.as_ref().is_some_and(|m| m.kind == "update") {
            return;
        }
        let open: Handler =
            Rc::new(|this, window, cx| this.open_dialog("update", json!({}), window, cx));
        self.toast_with(
            ToastKind::Info,
            argus_core::i18n::format(
                if state == "available" {
                    "updates.availableToast"
                } else {
                    "updates.readyToast"
                },
                &[("version", &version)],
            ),
            10000,
            vec![(label("updates.view"), open)],
        );
        cx.notify();
    }

    /// Places `parts` where the catalog text names them (`{name}`).
    /// Lays out a formatted text whose placeholders were filled with marker
    /// characters, putting each part where its marker is.
    pub(in crate::frontend) fn rich(
        &self,
        text: String,
        mut parts: Vec<(char, AnyElement)>,
    ) -> AnyElement {
        let mut rest = text;
        let mut row = div().flex().flex_wrap().items_center().gap_x(px(4.));
        loop {
            let next = parts
                .iter()
                .enumerate()
                .filter_map(|(index, (marker, _))| rest.find(*marker).map(|at| (at, index)))
                .min();
            let Some((at, index)) = next else {
                if !rest.is_empty() {
                    row = row.child(rest);
                }
                break;
            };
            let (marker, element) = parts.remove(index);
            if at > 0 {
                row = row.child(rest[..at].to_owned());
            }
            row = row.child(element);
            rest = rest[at + marker.len_utf8()..].to_owned();
        }
        row.into_any_element()
    }

    // ---- Onboarding -------------------------------------------------------------

    pub(in crate::frontend) fn onboarding_next(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let steps = values(&configuration()["onboardingSteps"]).len();
        let step = self
            .modal
            .as_ref()
            .and_then(|m| m.args["step"].as_u64())
            .unwrap_or(0) as usize;
        if step + 1 >= steps {
            self.preference("finishOnboarding", Value::Null, window, cx);
        } else {
            self.onboarding_go(step + 1, window, cx);
        }
    }

    fn onboarding_go(&mut self, step: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(modal) = &mut self.modal
            && modal.kind == "onboarding"
        {
            let from = modal.args["step"].as_u64().unwrap_or(0) as i64;
            modal.args["dir"] = json!((step as i64 - from).signum());
            modal.args["step"] = json!(step);
            let id = modal.id;
            self.focus_for(&format!("modal-{id}-primary"), cx)
                .focus(window, cx);
        }
        window.refresh();
        cx.notify();
    }

    fn onboarding_row(
        &self,
        title: String,
        hint: Option<String>,
        control: impl IntoElement,
    ) -> AnyElement {
        let p = self.palette;
        div()
            .flex()
            .items_center()
            .gap(px(16.))
            .min_h(px(56.))
            .px(px(14.))
            .py(px(10.))
            .border_1()
            .border_color(p.stroke)
            .rounded(px(6.))
            .bg(p.layer)
            .flex_shrink_0()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(title)
                    .when_some(hint, |d, hint| {
                        d.child(
                            div()
                                .mt(px(2.))
                                .text_size(px(12.))
                                .text_color(p.muted)
                                .child(hint),
                        )
                    }),
            )
            .child(div().flex_shrink_0().child(control))
            .into_any_element()
    }

    fn onboarding_heading(&self, glyph: &str, step: &str) -> AnyElement {
        let p = self.palette;
        div()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .child(
                div()
                    .size(px(44.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(10.))
                    .bg(p.accent.opacity(0.14))
                    .child(icon(glyph, p.accent).size(px(22.))),
            )
            .child(
                self.dialog_title(label(&format!("onboarding.{step}.title")))
                    .mt(px(12.))
                    .mb(px(6.)),
            )
            .child(
                div()
                    .text_color(p.muted)
                    .child(label(&format!("onboarding.{step}.body"))),
            )
            .into_any_element()
    }

    fn onboarding_note(&self, text: String, warning: bool) -> AnyElement {
        let p = self.palette;
        if warning {
            div()
                .flex()
                .gap(px(6.))
                .flex_shrink_0()
                .text_size(px(12.))
                .text_color(p.ink)
                .child(
                    div()
                        .mt(px(2.))
                        .child(icon("WarningIcon", p.io).size(px(14.))),
                )
                .child(div().flex_1().child(text))
                .into_any_element()
        } else {
            div()
                .flex_shrink_0()
                .text_size(px(12.))
                .line_height(relative(1.9))
                .text_color(p.muted)
                .child(text)
                .into_any_element()
        }
    }

    fn onboarding_list(&self, items: Vec<AnyElement>) -> AnyElement {
        let p = self.palette;
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .pl(px(6.))
            .flex_shrink_0()
            .children(items.into_iter().map(|item| {
                div()
                    .flex()
                    .items_start()
                    .gap(px(10.))
                    .child(
                        div()
                            .mt(px(7.))
                            .size(px(5.))
                            .flex_shrink_0()
                            .rounded_full()
                            .bg(p.accent),
                    )
                    .child(div().flex_1().min_w_0().child(item))
            }))
            .into_any_element()
    }

    /// A step's state line: a spinner while busy, a check when done.
    fn onboarding_state(&self, text: String, busy: bool, done: bool, hint: String) -> Div {
        let p = self.palette;
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .when(busy, |d| d.child(self.spinner(14., p.accent)))
                    .when(done && !busy, |d| {
                        d.child(icon("CheckIcon", p.accent).size(px(14.)))
                    })
                    .child(text),
            )
            .child(
                div()
                    .mt(px(2.))
                    .text_size(px(12.))
                    .text_color(p.muted)
                    .child(hint),
            )
    }

    fn state_row(&self, state: Div, control: Option<AnyElement>) -> AnyElement {
        let p = self.palette;
        div()
            .flex()
            .items_center()
            .gap(px(16.))
            .min_h(px(56.))
            .px(px(14.))
            .py(px(10.))
            .border_1()
            .border_color(p.stroke)
            .rounded(px(6.))
            .bg(p.layer)
            .flex_shrink_0()
            .child(state)
            .children(control)
            .into_any_element()
    }

    pub(in crate::frontend) fn onboarding_dialog(
        &self,
        dialog: Dialog,
        modal: &dialogs::Modal,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Dialog {
        let p = self.palette;
        let id = modal.id;
        let step = modal.args["step"].as_u64().unwrap_or(0) as usize;
        let dir = modal.args["dir"].as_i64().unwrap_or(0) as f32;
        let steps = values(&configuration()["onboardingSteps"]);
        let section = steps.get(step).and_then(Value::as_str).unwrap_or("tips");
        let last = step + 1 >= steps.len();
        let settings = self.data["settings"].clone();
        let mut body = div().flex().flex_col().gap(px(12.));
        match section {
            "welcome" => {
                body = body
                    .child(
                        div()
                            .flex()
                            .justify_center()
                            .pt(px(8.))
                            .pb(px(4.))
                            .child(img("argus/logo.svg").size(px(72.))),
                    )
                    .child(
                        self.dialog_title(label("onboarding.welcome.title"))
                            .text_size(px(24.))
                            .flex()
                            .justify_center(),
                    )
                    .child(
                        div()
                            .text_color(p.muted)
                            .child(label("onboarding.welcome.body")),
                    )
                    .child(
                        self.onboarding_row(
                            label("onboarding.welcome.language"),
                            None,
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .child(icon("GlobeIcon", p.muted))
                                .child(self.language_select("onboarding-language", cx)),
                        ),
                    );
            }
            "display" => {
                let positions = [("side", "NavSideIcon"), ("top", "NavTopIcon")];
                let nav = self.segments(
                    "onboarding-nav",
                    positions
                        .into_iter()
                        .map(|(value, glyph)| {
                            (
                                label(&format!("settings.navigation.{value}")),
                                Some(glyph),
                                Action::Preference("navPosition".into(), json!(value)),
                                (value == "top") == self.nav_top,
                            )
                        })
                        .collect(),
                    cx,
                );
                let intervals = [500u64, 1000, 2000, 5000];
                let current = self.interval;
                body = body
                    .child(self.onboarding_heading("ThemeIcon", "display"))
                    .child(self.onboarding_row(
                        label("onboarding.display.nav"),
                        Some(label("onboarding.display.navHint")),
                        nav,
                    ))
                    .child(self.onboarding_row(
                        label("onboarding.display.motion"),
                        Some(label("onboarding.display.motionHint")),
                        self.motion_toggle("onboarding-motion", window, cx),
                    ))
                    .child(self.onboarding_row(
                        label("onboarding.display.chart"),
                        Some(label("onboarding.display.chartHint")),
                        self.chart_select("onboarding-chart", window, cx),
                    ))
                    .child(
                        self.onboarding_row(
                            label("onboarding.display.interval"),
                            Some(label("onboarding.display.intervalHint")),
                            self.select(
                                "onboarding-interval",
                                label("onboarding.display.interval"),
                                intervals
                                    .iter()
                                    .map(|ms| {
                                        tr!("shell.interval.option", seconds = *ms as f64 / 1000.)
                                    })
                                    .collect(),
                                intervals.iter().position(|ms| *ms == current),
                                false,
                                move |this, index, window, cx| {
                                    this.act(Action::Interval(intervals[index]), window, cx)
                                },
                                cx,
                            ),
                        ),
                    );
            }
            "background" => {
                let hotkey_error = &settings["hotkeyError"];
                let problem = if text(hotkey_error, "kind") == "taken" {
                    Some(tr!(
                        "onboarding.background.hotkeyTaken",
                        shortcut = recorder::shortcut_text(text(hotkey_error, "shortcut"))
                    ))
                } else {
                    recorder::hotkey_error_text(hotkey_error).map(|problem| {
                        if text(hotkey_error, "kind") == "saveFailed" {
                            problem
                        } else {
                            tr!("onboarding.background.hotkeyProblem", problem = problem)
                        }
                    })
                };
                let keys = recorder::shortcut_keys(text(&settings, "hotkey"));
                body = body
                    .child(self.onboarding_heading("TrayIcon", "background"))
                    .child(self.onboarding_row(
                        label("onboarding.background.autostart"),
                        Some(label("onboarding.background.autostartHint")),
                        self.setting_toggle(
                            "onboarding-autostart",
                            boolean(&settings, "autostart"),
                            "set_autostart",
                            "enabled",
                            window,
                            cx,
                        ),
                    ))
                    .child(self.onboarding_row(
                        label("onboarding.background.closeToTray"),
                        Some(label("onboarding.background.closeToTrayHint")),
                        self.setting_toggle(
                            "onboarding-tray",
                            boolean(&settings, "closeToTray"),
                            "set_close_to_tray",
                            "value",
                            window,
                            cx,
                        ),
                    ))
                    .child(self.onboarding_row(
                        label("onboarding.background.minimizedInterval"),
                        Some(label("onboarding.background.minimizedIntervalHint")),
                        self.background_select("onboarding", "minimized", cx),
                    ))
                    .child(self.onboarding_row(
                        label("onboarding.background.trayInterval"),
                        Some(label("onboarding.background.trayIntervalHint")),
                        self.background_select("onboarding", "tray", cx),
                    ))
                    .child(match problem {
                        Some(problem) => self.onboarding_note(problem, true),
                        None if boolean(&settings, "hotkeyEnabled") && !keys.is_empty() => div()
                            .text_size(px(12.))
                            .text_color(p.muted)
                            .child(self.rich(
                                tr!("onboarding.background.hotkey", hotkey = ""),
                                vec![('', self.keycaps(&keys, false, 0.))],
                            ))
                            .into_any_element(),
                        None => {
                            self.onboarding_note(label("onboarding.background.hotkeyOff"), false)
                        }
                    });
            }
            "update" => {
                let save = |key: &'static str, value: Value| {
                    move |this: &mut Desktop, window: &mut Window, cx: &mut Context<Desktop>| {
                        this.preference(key, value.clone(), window, cx)
                    }
                };
                let channel = text(&settings, "updateChannel").to_owned();
                let routes = ["auto", "direct", "proxy"];
                let route = text(&settings, "updateRoute").to_owned();
                let disabled = self
                    .data
                    .get("updates")
                    .is_some_and(|s| text(s, "state") == "disabled");
                let save_check = save("updateCheck", Value::Null);
                let _ = save_check;
                body = body
                    .child(self.onboarding_heading("UpdateIcon", "update"))
                    .child(self.onboarding_row(
                        label("onboarding.update.check"),
                        Some(label("onboarding.update.checkHint")),
                        self.toggle(
                            "onboarding-check",
                            label("onboarding.update.check"),
                            boolean(&settings, "updateCheck"),
                            false,
                            |this, on, window, cx| {
                                this.preference("updateCheck", json!(on), window, cx)
                            },
                            window,
                            cx,
                        ),
                    ))
                    .child(
                        self.onboarding_row(
                            label("onboarding.update.channel"),
                            (channel == "beta").then(|| label("onboarding.update.betaHint")),
                            self.segments(
                                "onboarding-channel",
                                ["stable", "beta"]
                                    .into_iter()
                                    .map(|value| {
                                        (
                                            label(&format!("onboarding.update.{value}")),
                                            None,
                                            Action::Preference(
                                                "updateChannel".into(),
                                                json!(value),
                                            ),
                                            channel == value,
                                        )
                                    })
                                    .collect(),
                                cx,
                            ),
                        ),
                    )
                    .child(self.onboarding_row(
                        label("onboarding.update.route"),
                        None,
                        self.select(
                            "onboarding-route",
                            label("onboarding.update.route"),
                            vec![
                                label("onboarding.update.routeAuto"),
                                label("onboarding.update.routeDirect"),
                                label("onboarding.update.routeProxy"),
                            ],
                            routes.iter().position(|r| *r == route),
                            false,
                            move |this, index, window, cx| {
                                this.preference("updateRoute", json!(routes[index]), window, cx)
                            },
                            cx,
                        ),
                    ))
                    .child(self.onboarding_note(label("onboarding.update.proxyNotice"), false))
                    .when(disabled, |d| {
                        d.child(
                            self.onboarding_note(
                                self.data
                                    .get("updates")
                                    .and_then(|s| s["error"].as_str())
                                    .map(str::to_owned)
                                    .unwrap_or_else(|| label("onboarding.update.unavailable")),
                                false,
                            ),
                        )
                    });
            }
            "privilege" => {
                let status = self.data.get("privilege").cloned().unwrap_or(Value::Null);
                let busy = self.ui.busy.contains("privilege");
                let checking = !self.data.contains_key("privilege");
                let connected = widgets::service_connected(&status);
                let action = match text(&status, "state") {
                    "notInstalled" => Some("install"),
                    "stopped" if boolean(&status, "authorized") => Some("start"),
                    _ => None,
                };
                let state = if busy {
                    label("settings.privilege.state.busy")
                } else if checking {
                    label("settings.privilege.state.checking")
                } else {
                    widgets::service_state(&status)
                };
                let hint = if connected {
                    label("onboarding.privilege.replace")
                } else if action.is_some() || busy {
                    label("onboarding.privilege.later")
                } else {
                    label("onboarding.privilege.manage")
                };
                let control = action.map(|action| {
                    self.btn(
                        format!("onboarding-service-{action}"),
                        label(if action == "install" {
                            "settings.privilege.install.action"
                        } else {
                            "settings.privilege.start"
                        }),
                        None,
                        Tone::Plain,
                    )
                    .inert(busy || checking)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.service_action(action.into(), window, cx)
                    }))
                    .into_any_element()
                });
                body = body
                    .child(self.onboarding_heading("ShieldIcon", "privilege"))
                    .child(
                        self.onboarding_list(
                            ["end", "tune", "inspect", "control", "sensors"]
                                .into_iter()
                                .map(|f| {
                                    label(&format!("onboarding.privilege.features.{f}"))
                                        .into_any_element()
                                })
                                .collect(),
                        ),
                    )
                    .child(self.onboarding_note(label("onboarding.privilege.risk"), true))
                    .when(boolean(&self.data["info"], "elevated"), |d| {
                        d.child(self.onboarding_note(label("onboarding.privilege.elevated"), false))
                    })
                    .child(
                        self.state_row(
                            self.onboarding_state(state, busy, connected, hint),
                            control,
                        ),
                    )
                    .when_some(self.ui.errors.get("privilege").cloned(), |d, error| {
                        d.child(self.onboarding_note(error, true))
                    });
            }
            "sensors" => {
                let status = self.data.get("privilege").cloned().unwrap_or(Value::Null);
                let ready = widgets::service_connected(&status);
                let enabled = boolean(&settings, "kernelSensors");
                let busy = self.ui.busy.contains("sensors");
                let checking =
                    !self.data.contains_key("privilege") || self.ui.busy.contains("privilege");
                let state = if busy {
                    label("settings.sensors.preparing")
                } else if checking {
                    label("onboarding.sensors.checking")
                } else if enabled {
                    label("onboarding.sensors.enabled")
                } else {
                    label("onboarding.sensors.disabled")
                };
                let hint = if !ready {
                    label("onboarding.sensors.needsService")
                } else if enabled {
                    label("onboarding.sensors.manage")
                } else {
                    label("onboarding.sensors.later")
                };
                let privilege_step = steps
                    .iter()
                    .position(|s| s.as_str() == Some("privilege"))
                    .unwrap_or(0);
                let control = if !ready {
                    Some(
                        self.btn(
                            "onboarding-configure",
                            label("onboarding.sensors.configureService"),
                            None,
                            Tone::Plain,
                        )
                        .inert(busy || checking)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.onboarding_go(privilege_step, window, cx)
                        }))
                        .into_any_element(),
                    )
                } else if !enabled {
                    Some(
                        self.btn(
                            "onboarding-sensors",
                            label("onboarding.sensors.enable"),
                            None,
                            Tone::Plain,
                        )
                        .inert(busy || checking)
                        .on_click(
                            cx.listener(|this, _, window, cx| this.set_sensors(true, window, cx)),
                        )
                        .into_any_element(),
                    )
                } else {
                    None
                };
                body = body
                    .child(self.onboarding_heading("BoltIcon", "sensors"))
                    .child(self.onboarding_note(label("onboarding.sensors.install"), false))
                    .child(
                        self.state_row(self.onboarding_state(state, busy, enabled, hint), control),
                    )
                    .when_some(self.ui.errors.get("sensors").cloned(), |d, error| {
                        d.child(self.onboarding_note(error, true))
                    });
            }
            "taskManager" => {
                let service = self.data.get("privilege").cloned().unwrap_or(Value::Null);
                let ready = widgets::service_connected(&service);
                let status = self.data.get("taskmgr").cloned().unwrap_or(Value::Null);
                let busy = self.ui.busy.contains("taskmgr");
                let checking =
                    !self.data.contains_key("taskmgr") || !self.data.contains_key("privilege");
                let enabled = text(&status, "state") == "enabled";
                let state = if busy {
                    label("settings.taskManager.busy")
                } else if checking {
                    label("settings.taskManager.checking")
                } else if !ready {
                    label("settings.privilege.state.notInstalled")
                } else if status.is_null() {
                    label("settings.taskManager.unknown")
                } else {
                    label(&format!(
                        "settings.taskManager.state.{}",
                        text(&status, "state")
                    ))
                };
                let detail = status["detail"].as_str().map(str::to_owned);
                let hint = if !ready {
                    label("onboarding.taskManager.needsService")
                } else if enabled {
                    detail.clone().unwrap_or_default()
                } else if boolean(&status, "canEnable") || busy {
                    label("onboarding.taskManager.later")
                } else {
                    detail.unwrap_or_else(|| label("settings.taskManager.retry"))
                };
                let control = (ready && boolean(&status, "canEnable")).then(|| {
                    self.btn(
                        "onboarding-taskmgr",
                        label("settings.taskManager.enable"),
                        None,
                        Tone::Plain,
                    )
                    .inert(busy || checking)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.task_manager_action("enable".into(), window, cx)
                    }))
                    .into_any_element()
                });
                body = body
                    .child(self.onboarding_heading("KeyboardIcon", "taskManager"))
                    .child(self.onboarding_note(label("onboarding.taskManager.scope"), false))
                    .child(self.onboarding_note(label("onboarding.taskManager.undo"), false))
                    .child(
                        self.state_row(self.onboarding_state(state, busy, enabled, hint), control),
                    )
                    .when_some(self.ui.errors.get("taskmgr").cloned(), |d, error| {
                        d.child(self.onboarding_note(error, true))
                    });
            }
            _ => {
                let ctrl_digits = div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(self.keycaps(&["Ctrl".into(), "1".into()], false, 0.))
                    .child("–")
                    .child(self.keycaps(&["7".into()], false, 0.))
                    .into_any_element();
                body = body
                    .child(self.onboarding_heading("SparkleIcon", "tips"))
                    .child(self.onboarding_list(vec![
                        self.rich(
                            tr!("onboarding.tips.pages", keys = "", search = ""),
                            vec![
                                ('', ctrl_digits),
                                ('', self.keycaps(&["Ctrl".into(), "F".into()], false, 0.)),
                            ],
                        ),
                        label("onboarding.tips.menu").into_any_element(),
                        label("onboarding.tips.drop").into_any_element(),
                        label("onboarding.tips.history").into_any_element(),
                    ]))
                    .child(self.onboarding_note(label("onboarding.tips.reopen"), false));
            }
        }
        let height = px(540.)
            .min(window.viewport_size().height - px(140.))
            .max(px(200.));
        let reduce = cx.reduce_motion();
        let scroll = scrollbar::fresh_handle("onboarding-step", (id << 8) | step as u64);
        let content = div()
            .id(("onboarding-step", id))
            .track_scroll(&scroll)
            .h(height)
            .overflow_y_scroll()
            .pt(px(28.))
            .pl(px(32.))
            // The gutter is kept (`scrollbar-gutter: stable`).
            .pr(px(32. + scrollbar::SIZE))
            .pb(px(20.))
            .child(
                body.with_animation(
                    ("onboarding-slide", step),
                    Animation::new(Duration::from_millis(if reduce || dir == 0. {
                        1
                    } else {
                        250
                    }))
                    .with_easing(theme::ease_out),
                    move |body, delta| {
                        body.relative()
                            .left(px(dir * 24. * (1. - delta)))
                            .opacity(if dir == 0. { 1. } else { delta })
                    },
                ),
            )
            .child(scrollbar::scrollbar(
                "onboarding-scrollbar",
                &scroll,
                p.dark,
            ));
        let dots = div()
            .flex()
            .gap(px(6.))
            .mr_auto()
            .children((0..steps.len()).map(|index| {
                let width = gpui_kit::base::motion::transition(
                    format!("onboarding-dot-{index}"),
                    if index == step { 18f32 } else { 6. },
                    theme::fluent(167, theme::ease_move),
                    window,
                    cx,
                );
                div()
                    .w(px(width))
                    .h(px(6.))
                    .rounded(px(3.))
                    .bg(if index == step {
                        p.accent
                    } else {
                        p.stroke_strong
                    })
            }));
        dialog.child(self.measure(id, 0).child(content)).footer(
            self.measure(id, 1).child(
                self.dialog_footer()
                    .child(dots)
                    .when(!last, |d| {
                        d.child(
                            self.btn(
                                format!("modal-{id}-skip"),
                                label("onboarding.skip"),
                                None,
                                Tone::Ghost,
                            )
                            .on_click(cx.listener(
                                |this, _, window, cx| {
                                    this.preference("finishOnboarding", Value::Null, window, cx)
                                },
                            )),
                        )
                    })
                    .when(step > 0, |d| {
                        d.child(
                            self.btn(
                                format!("modal-{id}-back"),
                                label("onboarding.back"),
                                None,
                                Tone::Plain,
                            )
                            .min_w(px(96.))
                            .on_click(cx.listener(
                                move |this, _, window, cx| this.onboarding_go(step - 1, window, cx),
                            )),
                        )
                    })
                    .child(
                        self.btn(
                            format!("modal-{id}-next"),
                            label(if last {
                                "onboarding.finish"
                            } else {
                                "onboarding.next"
                            }),
                            None,
                            Tone::Accent,
                        )
                        .min_w(px(96.))
                        .track_focus(&self.focus_for(&format!("modal-{id}-primary"), cx))
                        .on_click(
                            cx.listener(|this, _, window, cx| this.onboarding_next(window, cx)),
                        ),
                    ),
            ),
        )
    }

    // ---- Update dialog and release reader -----------------------------------

    pub(in crate::frontend) fn update_head(&self, title: String, subtitle: String) -> AnyElement {
        let p = self.palette;
        div()
            .flex()
            .items_center()
            .gap(px(14.))
            .child(img("argus/logo.svg").size(px(40.)).flex_shrink_0())
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(self.dialog_title(title))
                    .when(!subtitle.is_empty(), |d| {
                        d.child(div().text_size(px(12.)).text_color(p.muted).child(subtitle))
                    }),
            )
            .into_any_element()
    }

    /// The channel and release date of `version` ("Stable · Released Oct 22").
    pub(in crate::frontend) fn update_subtitle(
        &self,
        status: &Value,
        target: &str,
        offer: bool,
    ) -> String {
        let releases = if offer {
            offered_releases(status)
        } else {
            changelog()
        };
        let wanted = version(target);
        let date = releases
            .iter()
            .find(|r| wanted.is_some() && version(&r.version) == wanted)
            .map(|r| r.date.clone())
            .filter(|d| !d.is_empty());
        let beta = wanted.is_some_and(|v| !v.pre.is_empty());
        [
            offer.then(|| {
                label(if beta {
                    "updates.dialog.beta"
                } else {
                    "updates.dialog.stable"
                })
            }),
            date.map(|date| tr!("updates.dialog.released", date = date)),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ")
    }

    /// This version beside the next one (ReleaseNotes.tsx, UpdateVersions).
    pub(in crate::frontend) fn update_versions(
        &self,
        from: Option<&str>,
        to: &str,
        installed: bool,
    ) -> AnyElement {
        let p = self.palette;
        let Some(from) = from.filter(|f| !f.is_empty() && !to.is_empty()) else {
            return div().into_any_element();
        };
        let tile = |caption: &str, number: String, next: bool| {
            div()
                .flex_1()
                .min_w_0()
                .px(px(14.))
                .py(px(10.))
                .border_1()
                .border_color(if next {
                    p.accent.opacity(0.4)
                } else {
                    p.stroke
                })
                .rounded(px(6.))
                .bg(if next {
                    p.accent.opacity(0.08)
                } else {
                    p.layer
                })
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(p.muted)
                        .child(label(caption)),
                )
                .child(
                    div()
                        .text_size(px(16.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(if next { p.accent } else { p.ink })
                        .child(number),
                )
        };
        div()
            .flex()
            .items_center()
            .gap(px(10.))
            .child(tile(
                if installed {
                    "updates.dialog.previous"
                } else {
                    "updates.dialog.current"
                },
                from.to_owned(),
                false,
            ))
            .child(icon("ArrowRightIcon", p.muted))
            .child(tile(
                if installed {
                    "updates.dialog.current"
                } else {
                    "updates.dialog.next"
                },
                to.to_owned(),
                true,
            ))
            .into_any_element()
    }

    /// The releases between two versions, each folded under its heading.
    pub(in crate::frontend) fn release_reader(
        &self,
        status: &Value,
        from: Option<&str>,
        to: &str,
        open_all: bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.palette;
        let offer = !status.is_null();
        let all = if offer {
            offered_releases(status)
        } else {
            changelog()
        };
        let earlier = offer
            && !all.is_empty()
            && from.is_some_and(|from| all.iter().all(|r| version(&r.version) > version(from)));
        let entries = between(all, from, to);
        let reader = div().flex().flex_col().gap(px(6.));
        if entries.is_empty() {
            let notes = text(status, "notes");
            return if notes.is_empty() {
                reader
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(p.muted)
                            .child(label("updates.dialog.empty")),
                    )
                    .into_any_element()
            } else {
                reader
                    .child(gpui_kit::component::text::TextView::markdown(
                        "update-notes",
                        notes.to_owned(),
                    ))
                    .into_any_element()
            };
        }
        let folded = self
            .ui
            .errors
            .get("releaseFolds")
            .cloned()
            .unwrap_or_default();
        let mut reader = reader;
        for (index, release) in entries.iter().enumerate() {
            let key = format!("|{}|", release.version);
            let default_open = open_all || index == 0;
            let open = default_open != folded.contains(&key);
            let changes = release
                .notes
                .lines()
                .filter(|line| {
                    line.trim_start().starts_with("- ") || line.trim_start().starts_with("* ")
                })
                .count();
            reader = reader.child(
                div()
                    .flex()
                    .flex_col()
                    .border_1()
                    .border_color(p.stroke)
                    .rounded(px(6.))
                    .bg(p.layer)
                    .child(
                        gpui_kit::base::Button::new(SharedString::from(format!(
                            "release-{}",
                            release.version
                        )))
                        .w_full()
                        .h(px(36.))
                        .px(px(12.))
                        .gap(px(8.))
                        .justify_start()
                        .rounded(px(6.))
                        .hover(move |s| s.bg(p.hover))
                        .child(
                            icon(if open { "ChevronDown" } else { "ChevronRight" }, p.muted)
                                .size(px(12.)),
                        )
                        .child(
                            div()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(release.version.clone()),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(p.muted)
                                .child(release.date.clone()),
                        )
                        .when(changes > 0, |b| {
                            b.child(
                                div()
                                    .ml_auto()
                                    .text_size(px(12.))
                                    .text_color(p.muted)
                                    .child(tr!("updates.dialog.changes", count = changes)),
                            )
                        })
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                let folds =
                                    this.ui.errors.entry("releaseFolds".into()).or_default();
                                if folds.contains(&key) {
                                    *folds = folds.replace(&key, "");
                                } else {
                                    folds.push_str(&key);
                                }
                                window.refresh();
                                cx.notify();
                            },
                        )),
                    )
                    .when(open, |d| {
                        d.child(div().px(px(14.)).pb(px(12.)).text_size(px(13.)).child(
                            gpui_kit::component::text::TextView::markdown(
                                SharedString::from(format!("release-notes-{}", release.version)),
                                release.notes.clone(),
                            ),
                        ))
                    }),
            );
        }
        if earlier {
            reader = reader.child(
                div()
                    .text_size(px(12.))
                    .text_color(p.muted)
                    .child(label("updates.dialog.earlier")),
            );
        }
        reader.into_any_element()
    }

    /// The update dialog's main action: install a ready update, or open the
    /// release page for a portable copy.
    pub(in crate::frontend) fn update_primary(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let status = self.data.get("updates").cloned().unwrap_or(Value::Null);
        match text(&status, "state") {
            "ready" => {
                self.prefs
                    .set("update.from", text(&self.data["info"], "version"));
                self.install_update(window, cx);
            }
            "available" => {
                self.request(
                    "open_release_page",
                    json!({"version":status["version"]}),
                    None,
                    window,
                    cx,
                );
                self.close_overlay(window, cx);
            }
            _ => {}
        }
    }

    pub(in crate::frontend) fn update_dialog(
        &self,
        dialog: Dialog,
        modal: &dialogs::Modal,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Dialog {
        let p = self.palette;
        let id = modal.id;
        let offer = modal.kind == "update";
        let status = self.data.get("updates").cloned().unwrap_or(Value::Null);
        let state = if offer {
            text(&status, "state").to_owned()
        } else {
            String::new()
        };
        // The installed build: what the changelog inside it describes.
        let current = text(&self.data["info"], "version").to_owned();
        let current = if current.is_empty() || !offer {
            env!("CARGO_PKG_VERSION").to_owned()
        } else {
            current
        };
        let target = if offer {
            text(&status, "version").to_owned()
        } else {
            current.clone()
        };
        let from = if offer {
            Some(current.clone())
        } else {
            modal.args["from"].as_str().map(str::to_owned)
        };
        let starting = self.ui.busy.contains("install");
        let busy = starting || state == "installing";
        let title = if !offer {
            tr!("updates.dialog.installed", version = target)
        } else {
            match state.as_str() {
                "available" => tr!("updates.dialog.available", version = target),
                "checking" | "downloading" if !target.is_empty() => {
                    tr!("updates.dialog.downloading", version = target)
                }
                "checking" | "downloading" => label("settings.update.status.checking"),
                "installing" => label("updates.dialog.installing"),
                "ready" => tr!("updates.dialog.ready", version = target),
                other => label(&format!(
                    "settings.update.status.{}",
                    if other.is_empty() { "idle" } else { other }
                )),
            }
        };
        let subtitle = self.update_subtitle(&status, &target, offer);
        let can_install = offer && matches!(state.as_str(), "ready" | "installing");
        let can_download = offer && state == "available";
        let total = number(&status, "total");
        let received = number(&status, "received");
        let fact = |glyph: &str, text: String, color: Hsla| {
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .text_size(px(12.))
                .text_color(color)
                .child(icon(glyph, color).size(px(14.)))
                .child(text)
        };
        let mut facts = div().flex().flex_wrap().gap(px(16.));
        if offer {
            if total > 0. {
                facts = facts.child(fact(
                    "UpdateIcon",
                    if state == "downloading" {
                        tr!(
                            "updates.dialog.progress",
                            received = theme::bytes(received, false),
                            total = theme::bytes(total, false)
                        )
                    } else {
                        theme::bytes(total, false)
                    },
                    p.muted,
                ));
            }
            if can_install {
                facts = facts.child(fact(
                    "ShieldIcon",
                    label("updates.dialog.verified"),
                    p.network,
                ));
            }
            if let Some(source) = status["source"].as_str() {
                facts = facts.child(fact(
                    "GlobeIcon",
                    tr!("updates.dialog.source", source = source),
                    p.muted,
                ));
            }
        }
        let failure = self
            .ui
            .errors
            .get("install")
            .cloned()
            .or_else(|| (state == "error").then(|| text(&status, "error").to_owned()))
            .filter(|f| !f.is_empty());
        let reader_status = if offer { status.clone() } else { Value::Null };
        let target_open = target.clone();
        let mut footer = self.dialog_footer().child(
            self.btn(
                format!("modal-{id}-github"),
                label("updates.dialog.github"),
                Some("ExternalIcon"),
                Tone::Ghost,
            )
            .mr_auto()
            .inert(target.is_empty())
            .on_click(cx.listener(move |this, _, window, cx| {
                this.request(
                    "open_release_page",
                    json!({"version":target_open}),
                    None,
                    window,
                    cx,
                );
            })),
        );
        let primary = self.focus_for(&format!("modal-{id}-primary"), cx);
        if can_install {
            footer = footer.child(
                self.btn(format!("modal-{id}-install"), "", None, Tone::Accent)
                    .min_w(px(96.))
                    .track_focus(&primary)
                    .when(busy, |b| {
                        b.child(self.spinner(14., p.on_accent))
                            .child(label("updates.dialog.starting"))
                    })
                    .when(!busy, |b| b.child(label("updates.dialog.install")))
                    .inert(busy)
                    .on_click(cx.listener(|this, _, window, cx| this.update_primary(window, cx))),
            );
        }
        if can_download {
            footer = footer.child(
                self.btn(
                    format!("modal-{id}-download"),
                    label("updates.dialog.download"),
                    Some("ExternalIcon"),
                    Tone::Accent,
                )
                .min_w(px(96.))
                .track_focus(&primary)
                .on_click(cx.listener(|this, _, window, cx| this.update_primary(window, cx))),
            );
        }
        footer = if can_install || can_download {
            footer.child(
                self.btn(
                    format!("modal-{id}-later"),
                    label("updates.dialog.later"),
                    None,
                    Tone::Plain,
                )
                .min_w(px(96.))
                .inert(busy)
                .on_click(cx.listener(|this, _, window, cx| this.close_overlay(window, cx))),
            )
        } else {
            footer.child(
                self.btn(
                    format!("modal-{id}-close"),
                    label("updates.dialog.close"),
                    None,
                    Tone::Accent,
                )
                .min_w(px(96.))
                .track_focus(&primary)
                .on_click(cx.listener(|this, _, window, cx| this.close_overlay(window, cx))),
            )
        };
        let height = window.viewport_size().height - px(48.);
        dialog
            .max_h(height)
            .child(
                self.measure(id, 0).child(
                    div()
                        .pt(px(22.))
                        .px(px(24.))
                        .pb(px(20.))
                        .flex()
                        .flex_col()
                        .gap(px(14.))
                        .child(self.update_head(title, subtitle))
                        .child(self.update_versions(from.as_deref(), &target, !offer))
                        .when(offer, |d| {
                            d.child(facts).when(
                                matches!(state.as_str(), "downloading" | "checking"),
                                |d| {
                                    d.child(self.progress_bar(
                                        (total > 0.).then(|| (received / total).min(1.) as f32),
                                        cx,
                                    ))
                                },
                            )
                        })
                        .child(self.release_reader(
                            &reader_status,
                            from.as_deref(),
                            &target,
                            true,
                            window,
                            cx,
                        ))
                        .when(can_install, |d| {
                            d.child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(p.muted)
                                    .child(label("updates.dialog.hint")),
                            )
                        })
                        .when(can_download, |d| {
                            d.child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(p.muted)
                                    .child(label("updates.dialog.portable")),
                            )
                        })
                        .when_some(failure, |d, failure| {
                            d.child(div().text_size(px(12.)).text_color(p.danger).child(failure))
                        }),
                ),
            )
            .footer(self.measure(id, 1).child(footer))
    }
}
