//! Settings: cards grouped into sections, with a list of the sections on the
//! left that follows the scroll (`section_rail`).
//!
//! This module lays out the page and lists each section's cards; a card that
//! needs more than a switch or a choice (its controls, state and commands) is
//! in a submodule named for its section. `cards` holds the card frames and
//! controls the sections share (other pages build their cards with them
//! too), and `results` what the host's answers to settings commands change.
use super::section_rail::RailEntry;
use super::*;
use crate::frontend::widgets::Tone;

mod animation;
mod cards;
mod data;
mod hotkey;
mod language;
mod privilege;
mod results;
mod rules;
mod shortcuts;
mod startup;
mod update;

pub(super) const SECTIONS: [&str; 8] = [
    "language",
    "startup",
    "hotkey",
    "privilege",
    "motion",
    "data",
    "rules",
    "update",
];
fn section_icon(section: &str) -> &'static str {
    match section {
        "language" => "GlobeIcon",
        "startup" => "PowerIcon",
        "hotkey" => "KeyboardIcon",
        "privilege" => "ShieldIcon",
        "motion" => "SparkleIcon",
        "data" => "HistoryIcon",
        "rules" => "BoltIcon",
        "update" => "UpdateIcon",
        _ => "InfoIcon",
    }
}

/// A short local time for "last checked" (month-day hour:minute).
pub(super) fn short_time(ms: i64) -> String {
    let Ok(value) = time::OffsetDateTime::from_unix_timestamp(ms.div_euclid(1000)) else {
        return label("format.missing");
    };
    let offset = time::UtcOffset::current_local_offset().unwrap_or(time::UtcOffset::UTC);
    let value = value.to_offset(offset);
    format!(
        "{:02}-{:02} {:02}:{:02}",
        u8::from(value.month()),
        value.day(),
        value.hour(),
        value.minute()
    )
}

impl Desktop {
    /// Settings polls once on arrival; only the rule statistics follow.
    pub(in crate::frontend) fn poll_settings(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.ui.errors.get("settingsEpoch").map(String::as_str)
            != Some(&self.page_epoch.to_string())
        {
            self.ui
                .errors
                .insert("settingsEpoch".into(), self.page_epoch.to_string());
            self.request("history_usage", json!({}), Some("historyUsage"), window, cx);
            self.request(
                "task_manager_status",
                json!({}),
                Some("taskmgr"),
                window,
                cx,
            );
            self.request(
                "privileged_service_status",
                json!({}),
                Some("privilege"),
                window,
                cx,
            );
            self.request(
                "administrator_status",
                json!({}),
                Some("administrator"),
                window,
                cx,
            );
            self.request("update_status", json!({}), Some("updates"), window, cx);
        }
        self.request("rule_stats", json!({}), Some("ruleStats"), window, cx);
    }

    /// Opens Settings at a card, pointing it out.
    pub(in crate::frontend) fn locate_setting(
        &mut self,
        section: &str,
        item: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Leading to another page leads away from the file lock dialog too.
        if self.modal.as_ref().is_some_and(|m| m.kind == "filelock") {
            self.close_overlay(window, cx);
        }
        // The tray's settings are on the Components page, in its first section.
        if section == "components" {
            self.open_components("tray", window, cx);
        } else {
            self.preference("settingsSection", json!(section), window, cx);
        }
        self.ui.located = Some((item.to_owned(), Instant::now()));
    }

    pub(in crate::frontend) fn tick_settings(&mut self) -> bool {
        let mut changed = false;
        if self
            .ui
            .located
            .as_ref()
            .is_some_and(|(_, at)| at.elapsed() > Duration::from_millis(2000))
        {
            self.ui.located = None;
            changed = true;
        }
        if self
            .ui
            .focus_rule
            .as_ref()
            .is_some_and(|(_, at)| at.elapsed() > Duration::from_millis(1600))
        {
            self.ui.focus_rule = None;
            changed = true;
        }
        changed
    }

    pub(in crate::frontend) fn settings_view(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let settings = self.data["settings"].clone();
        if self.ui.located.is_some() || self.ui.focus_rule.is_some() {
            window.request_animation_frame();
        }
        let sections = SECTIONS
            .into_iter()
            .enumerate()
            .map(|(index, section)| {
                self.settings_section_view(index, section, &settings, window, cx)
            })
            .collect();
        let pending_update = self.data.get("updates").is_some_and(|s| {
            matches!(text(s, "state"), "ready" | "available") && s["version"].is_string()
        });
        let entries = SECTIONS
            .map(|section| RailEntry {
                key: section,
                name: label(&format!("settings.sections.{section}")),
                icon: section_icon(section),
                dot: (section == "update").then_some(pending_update),
            })
            .into();
        self.section_page(
            "settings",
            |this| &mut this.settings_rail,
            entries,
            sections,
            window,
            cx,
        )
    }

    fn settings_section_view(
        &mut self,
        index: usize,
        section: &str,
        settings: &Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.palette;
        let mut cards: Vec<AnyElement> = vec![];
        match section {
            "language" => {
                cards.push(self.setting(
                    "language",
                    self.glyph("GlobeIcon"),
                    label("settings.language.title"),
                    None,
                    None,
                    vec![self.language_select("settings-language", cx)],
                ));
                let themes = ["system", "light", "dark"]
                    .map(|value| (value, label(&format!("settings.theme.{value}"))));
                cards.push(self.setting(
                    "theme",
                    self.glyph("ThemeIcon"),
                    label("settings.theme.title"),
                    None,
                    None,
                    vec![self.setting_choices(
                        "theme",
                        text(settings, "theme"),
                        &themes,
                        Some("set_theme"),
                        cx,
                    )],
                ));
                let positions = [("side", "NavSideIcon"), ("top", "NavTopIcon")];
                cards.push(self.setting_text(
                    "navigation",
                    if self.nav_top {
                        "NavTopIcon"
                    } else {
                        "NavSideIcon"
                    },
                    "settings.navigation.title",
                    "settings.navigation.desc",
                    vec![self.segments(
                        "setting-navPosition",
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
                    )],
                ));
                cards.push(self.appearance_setting(
                    &appearance::SCALE,
                    "ui-scale",
                    "InterfaceScaleIcon",
                    "settings.appearance.scaleTitle",
                    "settings.appearance.scaleDesc",
                    "%",
                    window,
                    cx,
                ));
                cards.push(self.appearance_setting(
                    &appearance::FONT_SIZE,
                    "font-size",
                    "FontSizeIcon",
                    "settings.appearance.fontTitle",
                    "settings.appearance.fontDesc",
                    "px",
                    window,
                    cx,
                ));
                cards.push(self.setting_text(
                    "vitalsMode",
                    "GaugeIcon",
                    "settings.vitals.title",
                    "settings.vitals.desc",
                    vec![self.vitals_mode_setting(cx)],
                ));
                cards.push(self.setting_text(
                    "processVisibility",
                    "EyeOffIcon",
                    "settings.visibility.title",
                    "settings.visibility.desc",
                    vec![self.visibility_editor_button(cx)],
                ));
                cards.push(self.setting_text(
                    "groupByUser",
                    "UsersIcon",
                    "settings.userGroups.title",
                    "settings.userGroups.desc",
                    vec![self.user_grouping_setting(cx)],
                ));
                let hold = self.toggle(
                    "holdOrder",
                    label("settings.holdOrder.title"),
                    self.hover_holds_order(),
                    false,
                    |this, on, window, cx| {
                        this.preference(tables::HOLD_ORDER, json!(on), window, cx);
                    },
                    window,
                    cx,
                );
                cards.push(self.setting_text(
                    "holdOrder",
                    "OrderIcon",
                    "settings.holdOrder.title",
                    "settings.holdOrder.desc",
                    vec![hold],
                ));
                cards.push(self.leak_setting(window, cx));
            }
            "startup" => {
                let autostart = self.setting_toggle(
                    "autostart",
                    "settings.startup.autostart.title",
                    boolean(settings, "autostart"),
                    "set_autostart",
                    "enabled",
                    window,
                    cx,
                );
                cards.push(self.setting_text(
                    "autostart",
                    "PowerIcon",
                    "settings.startup.autostart.title",
                    "settings.startup.autostart.desc",
                    vec![autostart],
                ));
                cards.push(self.start_page_setting(cx));
                // The tray's settings are on the Components page.
                cards.push(self.components_link(cx));
            }
            "hotkey" => {
                cards.push(self.hotkey_card(settings, window, cx));
                let view = self
                    .btn(
                        "shortcuts",
                        label("settings.shortcuts.view"),
                        None,
                        Tone::Plain,
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.open_dialog("shortcuts", json!({}), window, cx)
                    }))
                    .into_any_element();
                cards.push(self.setting(
                    "shortcuts",
                    self.glyph("KeyboardIcon"),
                    label("settings.shortcuts.title"),
                    None,
                    None,
                    vec![view],
                ));
            }
            "privilege" => {
                cards.push(self.administrator_card(false, window, cx));
                cards.push(self.service_card(cx));
                if self.kernel_sensors_supported() {
                    cards.push(self.sensors_card(settings, window, cx));
                }
                cards.push(self.task_manager_card(cx));
            }
            "motion" => {
                let window_motion = self.toggle(
                    "window-motion",
                    label("settings.motion.window.title"),
                    crate::window_motion::enabled(),
                    false,
                    |this, on, window, cx| {
                        this.preference(
                            "windowMotion",
                            json!(if on { "on" } else { "off" }),
                            window,
                            cx,
                        );
                    },
                    window,
                    cx,
                );
                cards.push(self.setting_text(
                    "windowMotion",
                    "AppIcon",
                    "settings.motion.window.title",
                    "settings.motion.window.desc",
                    vec![window_motion],
                ));
                let motion = self.motion_toggle("ui-motion", window, cx);
                cards.push(self.setting_text(
                    "uiMotion",
                    "SparkleIcon",
                    "settings.motion.ui.title",
                    "settings.motion.ui.desc",
                    vec![motion],
                ));
                let chart = self.chart_select("chart-mode", window, cx);
                cards.push(self.setting_text(
                    "chartMode",
                    "PulseIcon",
                    "settings.motion.chart.title",
                    "settings.motion.chart.desc",
                    vec![chart],
                ));
                let acceleration = self.toggle(
                    "hardwareAcceleration",
                    label("settings.motion.acceleration.title"),
                    // Until the settings are read, as saved settings without it.
                    settings["hardwareAcceleration"].as_bool().unwrap_or(true),
                    false,
                    |this, on, window, cx| {
                        this.request(
                            "set_hardware_acceleration",
                            json!({"enabled": on}),
                            Some("settings"),
                            window,
                            cx,
                        );
                    },
                    window,
                    cx,
                );
                cards.push(self.setting_text(
                    "hardwareAcceleration",
                    "GpuIcon",
                    "settings.motion.acceleration.title",
                    "settings.motion.acceleration.desc",
                    vec![acceleration],
                ));
            }
            "data" => self.data_cards(settings, &mut cards, window, cx),
            "rules" => self.rule_cards(settings, &mut cards, window, cx),
            "update" => self.update_cards(settings, &mut cards, window, cx),
            _ => {}
        }
        div()
            .id(SharedString::from(format!("settings-{section}")))
            .w_full()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .gap(px(4.))
            .child(
                div()
                    .mt(px(if index == 0 { 6. } else { 28. }))
                    .mb(px(6.))
                    .text_size(theme::font(14.))
                    .font_weight(theme::LABEL_WEIGHT)
                    .text_color(p.ink)
                    .child(label(&format!("settings.sections.{section}"))),
            )
            .children(cards)
            .into_any_element()
    }
}
