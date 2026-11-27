//! Updates: the state of the offered release and what can be done with it,
//! the check schedule, channel, route and proxy, and the version and kind of
//! this build, which the About page and the tray show as well.
use super::*;
use crate::frontend::widgets::{ButtonExt, Field, Tone};

impl Desktop {
    // ---- Updates ----------------------------------------------------------------

    fn save_update(
        &mut self,
        key: &str,
        value: Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut settings = self.data["settings"].clone();
        settings[key] = value;
        self.request(
            "set_update_settings",
            json!({"check":settings["updateCheck"],"channel":settings["updateChannel"],"route":settings["updateRoute"],"proxy":settings["updateProxy"]}),
            Some("settings"),
            window,
            cx,
        );
    }

    fn save_proxy_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(input) = self.ui.inputs.get("proxyDraft").cloned() else {
            return;
        };
        let proxy = input.read(cx).value().trim().to_owned();
        if proxy.is_empty() || proxy == text(&self.data["settings"], "updateProxy") {
            return;
        }
        self.save_update("updateProxy", json!(proxy), window, cx);
    }

    pub(super) fn update_cards(
        &mut self,
        settings: &Value,
        cards: &mut Vec<AnyElement>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let p = self.palette;
        let status = self.data.get("updates").cloned().unwrap_or(Value::Null);
        let state = text(&status, "state").to_owned();
        let version = text(&status, "version").to_owned();
        let busy = matches!(state.as_str(), "checking" | "downloading" | "installing");
        let installing = self.ui.busy.contains("install");
        let total = number(&status, "total");
        let received = number(&status, "received");
        let title = if status.is_null() {
            label("settings.update.status.loading")
        } else if state == "downloading" && total > 0. {
            tr!(
                "settings.update.status.downloadingPercent",
                version = version,
                percent = ((received / total) * 100.).floor() as u64
            )
        } else {
            argus_core::i18n::format(
                &format!(
                    "settings.update.status.{}",
                    if state.is_empty() { "loading" } else { &state }
                ),
                &[("version", &version)],
            )
        };
        let source = status["source"].as_str().unwrap_or("GitHub").to_owned();
        let detail = match state.as_str() {
            _ if status.is_null() => String::new(),
            "disabled" => text(&status, "error").to_owned(),
            "ready" => label("settings.update.detail.ready"),
            "available" => label("settings.update.detail.portable"),
            "downloading" if total > 0. => tr!(
                "settings.update.detail.downloadedOf",
                received = theme::bytes(received, false),
                total = theme::bytes(total, false),
                source = source
            ),
            "downloading" => tr!(
                "settings.update.detail.downloaded",
                received = theme::bytes(received, false),
                source = source
            ),
            _ => match status["checkedAt"].as_i64() {
                Some(at) if status["source"].is_string() => tr!(
                    "settings.update.detail.checkedFrom",
                    time = short_time(at),
                    source = source
                ),
                Some(at) => tr!("settings.update.detail.checked", time = short_time(at)),
                None => label("settings.update.detail.source"),
            },
        };
        let current = tr!(
            "settings.update.current",
            version = self.shown_version(),
            build = self.build_label()
        );
        let offered = matches!(
            state.as_str(),
            "ready" | "available" | "downloading" | "installing"
        );
        let mut controls = vec![];
        // What the offered release brings opens in the update dialog.
        if offered && !version.is_empty() {
            controls.push(
                self.btn(
                    "update-notes",
                    label("updates.openDialog"),
                    Some("NotesIcon"),
                    Tone::Plain,
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.open_dialog("update", json!({}), window, cx)
                }))
                .into_any_element(),
            );
        }
        if state == "ready" {
            controls.push(
                self.btn(
                    "update-restart",
                    label("settings.update.restart"),
                    None,
                    Tone::Accent,
                )
                .inert(installing)
                .on_click(cx.listener(|this, _, window, cx| this.install_update(window, cx)))
                .into_any_element(),
            );
        }
        if state == "available" && !version.is_empty() {
            let version = version.clone();
            controls.push(
                self.btn(
                    "update-download",
                    label("settings.update.download"),
                    Some("ExternalIcon"),
                    Tone::Accent,
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.request(
                        "open_release_page",
                        json!({"version":version}),
                        None,
                        window,
                        cx,
                    );
                }))
                .into_any_element(),
            );
        }
        if !offered && state != "disabled" {
            controls.push(
                self.btn(
                    "check-update",
                    label("settings.update.check"),
                    None,
                    Tone::Plain,
                )
                .inert(status.is_null() || busy)
                .on_click(cx.listener(|this, _, window, cx| {
                    this.request("check_update", json!({}), None, window, cx);
                }))
                .into_any_element(),
            );
        }
        let progress = (state == "downloading").then(|| {
            div().mt(px(8.)).max_w(px(320.)).child(
                self.progress_bar((total > 0.).then(|| (received / total).min(1.) as f32), cx),
            )
        });
        cards.push(
            self.setting(
                "update",
                self.glyph("UpdateIcon"),
                title,
                Some(
                    div()
                        .flex()
                        .flex_col()
                        .child(current)
                        .when(!detail.is_empty(), |d| d.child(detail))
                        .children(progress)
                        .into_any_element(),
                ),
                if matches!(state.as_str(), "error" | "ready") {
                    status["error"].as_str().map(str::to_owned)
                } else {
                    None
                }
                .or_else(|| self.ui.errors.get("install").cloned()),
                controls,
            ),
        );
        let auto = self.toggle(
            "update-auto",
            label("settings.update.auto.title"),
            boolean(settings, "updateCheck"),
            false,
            |this, on, window, cx| this.save_update("updateCheck", json!(on), window, cx),
            window,
            cx,
        );
        cards.push(self.setting_text(
            "updateCheck",
            "TimerIcon",
            "settings.update.auto.title",
            "settings.update.auto.desc",
            vec![auto],
        ));
        let notify = self.setting_toggle(
            "updateNotify",
            "settings.update.notify.title",
            boolean(settings, "updateNotify"),
            "set_update_notify",
            "enabled",
            window,
            cx,
        );
        cards.push(self.setting_text(
            "updateNotify",
            "BellIcon",
            "settings.update.notify.title",
            "settings.update.notify.desc",
            vec![notify],
        ));
        let view = self
            .btn(
                "whats-new-view",
                label("settings.update.whatsNew.view"),
                None,
                Tone::Plain,
            )
            .on_click(cx.listener(|this, _, window, cx| {
                this.open_dialog("whatsnew", json!({}), window, cx)
            }))
            .into_any_element();
        let whats_new = self.setting_toggle(
            "whatsNew",
            "settings.update.whatsNew.title",
            boolean(settings, "whatsNew"),
            "set_whats_new",
            "enabled",
            window,
            cx,
        );
        cards.push(self.setting_text(
            "whatsNew",
            "NotesIcon",
            "settings.update.whatsNew.title",
            "settings.update.whatsNew.desc",
            vec![view, whats_new],
        ));
        let channels = ["stable", "beta"];
        let channel = text(settings, "updateChannel").to_owned();
        let channel_select = self.select(
            "update-channel",
            label("settings.update.channel.label"),
            channels
                .iter()
                .map(|c| label(&format!("settings.update.channel.{c}")))
                .collect(),
            channels.iter().position(|c| *c == channel),
            false,
            move |this, index, window, cx| {
                this.save_update("updateChannel", json!(channels[index]), window, cx)
            },
            cx,
        );
        cards.push(self.setting_text(
            "updateChannel",
            "SparkleIcon",
            "settings.update.channel.label",
            "settings.update.channel.desc",
            vec![channel_select],
        ));
        let routes = ["auto", "direct", "proxy"];
        let route = text(settings, "updateRoute").to_owned();
        let route_select = self.select(
            "update-route",
            label("settings.update.route.label"),
            routes
                .iter()
                .map(|r| label(&format!("settings.update.route.{r}")))
                .collect(),
            routes.iter().position(|r| *r == route),
            false,
            move |this, index, window, cx| {
                this.save_update("updateRoute", json!(routes[index]), window, cx)
            },
            cx,
        );
        cards.push(self.setting_text(
            "updateRoute",
            "GlobeIcon",
            "settings.update.route.label",
            "settings.update.route.desc",
            vec![route_select],
        ));
        if route != "direct" {
            let proxy = text(settings, "updateProxy").to_owned();
            let presets = crate::updater::PROXY_PRESETS;
            let custom = self.ui.proxy_draft.is_some() || !presets.contains(&proxy.as_str());
            let mut options = presets
                .iter()
                .map(|preset| {
                    url::Url::parse(preset)
                        .ok()
                        .and_then(|u| u.host_str().map(str::to_owned))
                        .unwrap_or_else(|| (*preset).to_owned())
                })
                .collect::<Vec<_>>();
            options.push(label("settings.update.proxy.custom"));
            let selected = if custom {
                presets.len()
            } else {
                presets.iter().position(|p| *p == proxy).unwrap_or(0)
            };
            let proxy_select = self.select(
                "update-proxy",
                label("settings.update.proxy.label"),
                options,
                Some(selected),
                false,
                move |this, index, window, cx| {
                    if index >= presets.len() {
                        let current = text(&this.data["settings"], "updateProxy").to_owned();
                        let draft = if presets.contains(&current.as_str()) {
                            "https://".to_owned()
                        } else {
                            current
                        };
                        this.ui.inputs.remove("proxyDraft");
                        this.ui.proxy_draft = Some(draft);
                    } else {
                        this.ui.proxy_draft = None;
                        this.ui.inputs.remove("proxyDraft");
                        this.save_update("updateProxy", json!(presets[index]), window, cx);
                    }
                    cx.notify();
                },
                cx,
            );
            cards.push(self.setting_text(
                "updateProxy",
                "TransferIcon",
                "settings.update.proxy.label",
                if route == "auto" {
                    "settings.update.proxy.fallback"
                } else {
                    "settings.update.proxy.only"
                },
                vec![proxy_select],
            ));
            if custom {
                let initial = self.ui.proxy_draft.clone().unwrap_or_else(|| proxy.clone());
                let input = self.inline_input(
                    "proxyDraft",
                    &initial,
                    |this, window, cx| this.save_proxy_draft(window, cx),
                    window,
                    cx,
                );
                let draft = input.read(cx).value().trim().to_owned();
                let form = div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(div().w(px(220.)).child(self.text_field(
                        &input,
                        widgets::input(&input),
                        Field::Boxed,
                        label("settings.update.custom.label").into(),
                        true,
                        window,
                        cx,
                    )))
                    .child(
                        self.btn(
                            "proxy-save",
                            label("settings.update.custom.save"),
                            None,
                            Tone::Plain,
                        )
                        .inert(draft.is_empty() || draft == proxy)
                        .on_click(
                            cx.listener(|this, _, window, cx| this.save_proxy_draft(window, cx)),
                        ),
                    )
                    .into_any_element();
                cards.push(self.setting_text(
                    "customProxy",
                    "GlobeIcon",
                    "settings.update.custom.title",
                    "settings.update.custom.desc",
                    vec![form],
                ));
            }
        }
        let star = self.toggle(
            "star-reminder",
            label("settings.update.star.title"),
            self.prefs.get(star::REMIND, true),
            false,
            |this, on, window, cx| this.preference(star::REMIND, json!(on), window, cx),
            window,
            cx,
        );
        cards.push(self.setting_text(
            "starReminder",
            "StarIcon",
            "settings.update.star.title",
            "settings.update.star.desc",
            vec![star],
        ));
        let _ = p;
    }

    /// Starts the downloaded installer; a declined prompt is no failure.
    pub(in crate::frontend) fn install_update(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.ui.busy.contains("install") || self.options.fixture.is_some() {
            return;
        }
        self.ui.busy.insert("install".into());
        self.ui.errors.remove("install");
        self.request("install_update", json!({}), None, window, cx);
    }

    /// The version shown for this build: a test build's names its commit
    /// (`0.3.4+583.g4c638d7`); comparisons use `info.version`.
    pub(in crate::frontend) fn shown_version(&self) -> &str {
        let info = &self.data["info"];
        Some(text(info, "build"))
            .filter(|build| !build.is_empty())
            .unwrap_or_else(|| text(info, "version"))
    }

    pub(in crate::frontend) fn build_label(&self) -> String {
        use argus_app::config::{BuildKind, build_kind};
        let kind = match text(&self.data["info"], "buildKind") {
            "development" => BuildKind::Development,
            "test" => BuildKind::Test,
            "stable" => BuildKind::Stable,
            _ => build_kind(self.shown_version(), cfg!(debug_assertions)),
        };
        label(kind.label_key())
    }
}
