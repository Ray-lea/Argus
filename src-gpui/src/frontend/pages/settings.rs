//! Settings: cards grouped into sections, with a list of the sections on the
//! left that follows the scroll (Settings.tsx and pages.css in the original).
use super::*;
use crate::frontend::recorder::{self, Effect, Mods, Verdict};
use crate::frontend::recorder::{hotkey_error_text, issue_text};
use crate::frontend::widgets::{
    ButtonExt, Field, HotkeyMessage, HotkeyMode, ToastKind, Tone, service_connected, service_state,
};
use gpui_kit::base::ElementExt;

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
const RAIL: f32 = 180.;
const GAP: f32 = 32.;
const CONTENT: f32 = 780.;
const NARROW: f32 = 900.;
const RULE_PRIORITIES: [&str; 6] = [
    "realtime",
    "high",
    "aboveNormal",
    "normal",
    "belowNormal",
    "idle",
];
/// Settings in the update section, by the setting a notice points to.
const HOTKEY_IDLE_MS: u64 = 15_000;
const HOTKEY_QUIET_MS: u64 = 1_500;
const HOTKEY_DONE_MS: u64 = 1_600;

/// A short local time for "last checked" (month-day hour:minute).
fn short_time(ms: i64) -> String {
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

/// What the administrator card shows: locale keys for its state and notes,
/// the state's tone and its controls (Settings.tsx, AdministratorSettings).
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
    /// A card glyph at the cards' 20 px.
    fn glyph(&self, name: &str) -> AnyElement {
        icon(name, self.palette.ink)
            .size(px(20.))
            .into_any_element()
    }

    /// The outline a card pointed out from elsewhere wears for a moment.
    fn located_tint(&self, id: &str) -> f32 {
        self.ui
            .located
            .as_ref()
            .filter(|(target, _)| target == id)
            .map(|(_, at)| 1. - theme::ease_out((at.elapsed().as_secs_f32() / 1.8).min(1.)))
            .unwrap_or(0.)
    }

    /// A located card's border and fill (pages.css `setting-located`): eased
    /// from the accent and from 12 % accent back to the card's own.
    fn located_colors(&self, tint: f32) -> (Hsla, Hsla) {
        let p = self.palette;
        if tint <= 0. {
            return (p.stroke, p.layer);
        }
        (
            theme::mix(p.stroke, p.accent, tint),
            theme::mix(p.layer, theme::mix(p.layer, p.accent, 0.12), tint),
        )
    }

    /// A located card's 1 px accent ring just outside its border (the
    /// keyframe's `box-shadow: 0 0 0 1px`), fading with it. Absolute inside
    /// the card: a GPUI shadow would tint the translucent card under it.
    fn located_ring(&self, tint: f32) -> Option<AnyElement> {
        (tint > 0.).then(|| {
            div()
                .absolute()
                .top(px(-2.))
                .left(px(-2.))
                .right(px(-2.))
                .bottom(px(-2.))
                .rounded(px(7.))
                .border_1()
                .border_color(self.palette.accent.opacity(tint))
                .into_any_element()
        })
    }

    /// One setting (`.setting`): icon, title and description, controls on the right.
    pub(in crate::frontend) fn setting(
        &self,
        id: &str,
        glyph: AnyElement,
        title: impl IntoElement,
        desc: Option<AnyElement>,
        error: Option<String>,
        controls: Vec<AnyElement>,
    ) -> AnyElement {
        let p = self.palette;
        let tint = self.located_tint(id);
        let (border, fill) = self.located_colors(tint);
        div()
            .id(SharedString::from(format!("setting-{id}")))
            .flex()
            .items_center()
            .gap(px(16.))
            .min_h(px(68.))
            .px(px(16.))
            .py(px(12.))
            .border_1()
            .border_color(border)
            .rounded(px(6.))
            .bg(fill)
            .children(self.located_ring(tint))
            .child(
                div()
                    .w(px(20.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(glyph),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(div().child(title))
                    .when_some(desc, |d, desc| {
                        d.child(
                            div()
                                .mt(px(2.))
                                .text_size(theme::font(12.))
                                .text_color(p.muted)
                                .child(desc),
                        )
                    })
                    .when_some(error, |d, error| {
                        d.child(
                            div()
                                .mt(px(4.))
                                .text_size(theme::font(12.))
                                .text_color(p.danger)
                                .child(error),
                        )
                    }),
            )
            .when(!controls.is_empty(), |d| {
                d.child(
                    div()
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .children(controls),
                )
            })
            .into_any_element()
    }

    fn setting_text(
        &self,
        id: &str,
        glyph: &str,
        title: &str,
        desc: &str,
        controls: Vec<AnyElement>,
    ) -> AnyElement {
        self.setting(
            id,
            self.glyph(glyph),
            label(title),
            Some(label(desc).into_any_element()),
            None,
            controls,
        )
    }

    /// One feature, one card (FeatureCard): what it is and its state on top;
    /// below a divider, what it reports beside what can be done.
    #[allow(clippy::too_many_arguments)]
    fn feature(
        &self,
        id: &str,
        glyph: &str,
        title: String,
        desc: String,
        state: String,
        tone: &str,
        note: AnyElement,
        error: Option<String>,
        controls: Vec<AnyElement>,
        footer: Option<String>,
    ) -> AnyElement {
        let p = self.palette;
        let tint = self.located_tint(id);
        let (border, fill) = self.located_colors(tint);
        div()
            .id(SharedString::from(format!("setting-{id}")))
            .flex()
            .flex_col()
            .border_1()
            .border_color(border)
            .rounded(px(6.))
            .bg(fill)
            .children(self.located_ring(tint))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(16.))
                    .px(px(16.))
                    .py(px(14.))
                    .child(div().w(px(20.)).flex_shrink_0().child(self.glyph(glyph)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(title)
                            .child(
                                div()
                                    .mt(px(2.))
                                    .text_size(theme::font(12.))
                                    .text_color(p.muted)
                                    .child(desc),
                            ),
                    )
                    .child(self.state_pill(state, tone)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(16.))
                    .min_h(px(52.))
                    .pl(px(52.))
                    .pr(px(16.))
                    .py(px(8.))
                    .border_t_1()
                    .border_color(p.stroke)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(theme::font(12.))
                            .text_color(p.muted)
                            .child(note)
                            .when_some(error, |d, error| {
                                d.child(div().mt(px(4.)).text_color(p.danger).child(error))
                            }),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .children(controls),
                    ),
            )
            .when_some(footer, |d, footer| {
                d.child(
                    div()
                        .pl(px(52.))
                        .pr(px(16.))
                        .pb(px(12.))
                        .text_size(theme::font(12.))
                        .text_color(p.faint)
                        .child(footer),
                )
            })
            .into_any_element()
    }

    /// A settings toggle running a command with `{argument: !value}`.
    pub(in crate::frontend) fn setting_toggle(
        &self,
        id: &str,
        value: bool,
        command: &str,
        argument: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let command = command.to_owned();
        let argument = argument.to_owned();
        self.toggle(
            id.to_owned(),
            id.to_owned(),
            value,
            false,
            move |this, on, window, cx| {
                this.request(
                    &command,
                    json!({argument.as_str(): on}),
                    Some("settings"),
                    window,
                    cx,
                );
            },
            window,
            cx,
        )
    }

    /// A segmented choice that saves a command or a preference.
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

    /// The language choice (also used by the onboarding).
    pub(in crate::frontend) fn language_select(
        &self,
        id: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let values = ["system", "zh-CN", "en-US"];
        let current = text(&self.data["settings"], "language");
        self.select(
            id.to_owned(),
            label("settings.language.title"),
            vec![
                label("common.languageSystem"),
                label("common.languageName.zhCN"),
                label("common.languageName.enUS"),
            ],
            values.iter().position(|v| *v == current),
            false,
            move |this, index, window, cx| {
                this.request(
                    "set_language",
                    json!({"language":values[index]}),
                    Some("settings"),
                    window,
                    cx,
                );
            },
            cx,
        )
    }

    /// The interface motion toggle (also used by the onboarding).
    pub(in crate::frontend) fn motion_toggle(
        &self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.toggle(
            id.to_owned(),
            label("settings.motion.ui.title"),
            !cx.reduce_motion(),
            false,
            |this, on, window, cx| {
                this.preference("uiMotion", json!(if on { "on" } else { "off" }), window, cx);
            },
            window,
            cx,
        )
    }

    /// The chart animation choice, labelled with frame rates (also used by the onboarding).
    pub(in crate::frontend) fn chart_select(
        &self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let fps = |mode| {
            (1. / motion::frame_period(mode, window.refresh_interval(cx)).as_secs_f64()).round()
                as u32
        };
        let modes = ["fluid", "balanced", "pixel", "off"];
        let current = self
            .prefs
            .get::<Option<String>>("motion.chartMode", None)
            .unwrap_or_else(|| {
                if crate::platform::system_motion() {
                    "fluid"
                } else {
                    "off"
                }
                .into()
            });
        self.select(
            id.to_owned(),
            label("settings.motion.chart.title"),
            vec![
                tr!(
                    "settings.motion.chart.fluid",
                    fps = fps(motion::Mode::Fluid)
                ),
                tr!(
                    "settings.motion.chart.balanced",
                    fps = fps(motion::Mode::Balanced)
                ),
                label("settings.motion.chart.pixel"),
                label("settings.motion.chart.off"),
            ],
            modes.iter().position(|m| *m == current),
            false,
            move |this, index, window, cx| {
                this.preference("motion.chartMode", json!(modes[index]), window, cx);
            },
            cx,
        )
    }

    /// How often to sample while minimized or in the tray; under 5 s only
    /// after a warning (Settings.tsx, BackgroundIntervalSelect).
    pub(in crate::frontend) fn background_select(
        &self,
        place: &str,
        away: &'static str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let settings = &self.data["settings"];
        let field = if away == "minimized" {
            "minimizedInterval"
        } else {
            "trayInterval"
        };
        let current = settings[field].as_u64();
        let choices = crate::settings::BACKGROUND_INTERVALS;
        self.select(
            format!("{place}-background-{away}"),
            label(&format!("settings.data.{away}.title")),
            choices
                .iter()
                .map(|ms| tr!("shell.interval.option", seconds = *ms as f64 / 1000.))
                .collect(),
            choices.iter().position(|ms| Some(*ms) == current),
            false,
            move |this, index, window, cx| {
                let ms = choices[index];
                let save = move |this: &mut Desktop, window: &mut Window, cx: &mut Context<Desktop>| {
                    let settings = &this.data["settings"];
                    let mut args = json!({"minimized":settings["minimizedInterval"],"tray":settings["trayInterval"]});
                    args[away] = json!(ms);
                    this.request("set_background_intervals", args, Some("settings"), window, cx);
                };
                if ms >= 5000 {
                    save(this, window, cx);
                } else {
                    let seconds = ms as f64 / 1000.;
                    this.ask(
                        label(&format!("shell.background.confirm.{away}.title")),
                        argus_core::i18n::format(
                            &format!("shell.background.confirm.{away}.body"),
                            &[("seconds", &seconds)],
                        ),
                        tr!("shell.background.confirm.accept", seconds = seconds),
                        false,
                        save,
                        window,
                        cx,
                    );
                }
            },
            cx,
        )
    }

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

    /// Moves the section list's mark with the scroll, unless a section was
    /// just picked; plays a picked section's smooth scroll.
    fn follow_settings_scroll(&mut self, window: &mut Window, cx: &App) {
        let handle = &self.settings_scroll;
        let container = handle.bounds();
        if let Some(index) = self.settings_target.take() {
            // Wait for one layout after the request: the page may be arriving,
            // or its text may have reflowed (a language change) since the last.
            let fresh = std::mem::replace(&mut self.ui.settings_jump_wait, false);
            self.settings_section = SECTIONS[index].into();
            self.ui.settings_picked = true;
            match handle.bounds_for_item(index) {
                Some(child) if fresh && container.size.height > px(0.) => {
                    let target = (container.top() - child.top())
                        .max(-handle.max_offset().y)
                        .min(px(0.));
                    handle.set_offset(point(px(0.), target));
                    self.ui.settings_scroll = None;
                    self.ui.settings_picked = true;
                    self.settings_section = SECTIONS[index].into();
                }
                _ => {
                    self.settings_target = Some(index);
                    self.ui.settings_jump_wait = true;
                    window.request_animation_frame();
                }
            }
        }
        if let Some((from, to, start)) = self.ui.settings_scroll {
            let t = (start.elapsed().as_secs_f32() / 0.3).min(1.);
            let y = from
                + (to - from)
                    * if cx.reduce_motion() {
                        1.
                    } else {
                        theme::ease_out(t)
                    };
            handle.set_offset(point(px(0.), y));
            if t >= 1. || cx.reduce_motion() {
                self.ui.settings_scroll = None;
            } else {
                window.request_animation_frame();
            }
        }
        if self.ui.settings_picked || container.size.height <= px(0.) {
            return;
        }
        let h = f32::from(container.size.height);
        let y = -f32::from(handle.offset().y);
        let max = f32::from(handle.max_offset().y);
        let start = (y / (h / 2.)).min(1.);
        let end = (1. - (max - y) / (h / 2.)).max(0.);
        let line = h * (0.1 + 0.25 * start + 0.6 * end);
        let mut current = 0;
        for index in 0..SECTIONS.len() {
            if let Some(child) = handle.bounds_for_item(index)
                && f32::from(child.top() - container.top()) - y <= line
            {
                current = index;
            }
        }
        if y >= max - 4. && max > 0. {
            current = SECTIONS.len() - 1;
        }
        self.settings_section = SECTIONS[current].into();
    }

    /// Scrolls smoothly to a section picked in the list; it stays marked
    /// until the page is scrolled by hand.
    fn jump_to_section(&mut self, index: usize, cx: &App) {
        self.settings_section = SECTIONS[index].into();
        self.ui.settings_picked = true;
        let handle = &self.settings_scroll;
        let Some(child) = handle.bounds_for_item(index) else {
            return;
        };
        let container = handle.bounds();
        let target = (container.top() - child.top())
            .max(-handle.max_offset().y)
            .min(px(0.));
        if cx.reduce_motion() {
            handle.set_offset(point(px(0.), target));
        } else {
            self.ui.settings_scroll = Some((handle.offset().y, target, Instant::now()));
        }
    }

    /// Opens Settings at a card, pointing it out (Settings.tsx, goToSetting).
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
        self.preference("settingsSection", json!(section), window, cx);
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
        if !SECTIONS.contains(&self.settings_section.as_str()) {
            self.settings_section = SECTIONS[0].into();
        }
        self.follow_settings_scroll(window, cx);
        if self.ui.located.is_some() || self.ui.focus_rule.is_some() {
            window.request_animation_frame();
        }
        let p = self.palette;
        let width = f32::from(self.ui.settings_width.get());
        let width = if width > 0. {
            width
        } else {
            f32::from(window.viewport_size().width) - 24.
        };
        // The gutter is kept (`scrollbar-gutter: stable`): the container query
        // and the centring see the width beside it.
        let width = width - scrollbar::SIZE;
        let narrow = width <= NARROW;
        let layout = if narrow {
            (width - 8.).max(0.)
        } else {
            (width - 8.).min(RAIL + GAP + CONTENT)
        };
        let left = (width - layout) / 2.;
        let content_left = left + if narrow { 0. } else { RAIL + GAP };
        let content_width = layout - if narrow { 0. } else { RAIL + GAP };
        let sections = SECTIONS
            .into_iter()
            .enumerate()
            .map(|(index, section)| {
                self.settings_section_view(index, section, &settings, window, cx)
            })
            .collect::<Vec<_>>();
        let body = div()
            .id("settings-content")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.settings_scroll)
            .flex()
            .flex_col()
            .pl(px(content_left))
            .pr(px(
                (width - content_left - content_width).max(0.) + scrollbar::SIZE
            ))
            .pt(px(4.))
            .pb(px(32.))
            .children(sections)
            .on_scroll_wheel(cx.listener(|this, _, _, _| this.ui.settings_picked = false))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.ui.settings_picked = false),
            );
        let body = body.child(scrollbar::scrollbar(
            "settings-scrollbar",
            &self.settings_scroll,
            p.dark,
        ));
        let selected = SECTIONS
            .iter()
            .position(|s| *s == self.settings_section)
            .unwrap_or(0);
        let mark = gpui_kit::base::motion::transition(
            "settings-nav-mark",
            selected as f32 * 38.,
            theme::fluent(250, theme::ease_move),
            window,
            cx,
        );
        let pending_update = self.data.get("updates").is_some_and(|s| {
            matches!(text(s, "state"), "ready" | "available") && s["version"].is_string()
        });
        let rail = div()
            .absolute()
            .top(px(4.))
            .left(px(left))
            .w(px(RAIL))
            .flex()
            .flex_col()
            .gap(px(2.))
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(px(mark))
                    .h(px(36.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(p.stroke)
                    .bg(p.layer_strong)
                    .child(
                        div()
                            .absolute()
                            .left_0()
                            .top(px(8.))
                            .w(px(3.))
                            .h(px(18.))
                            .rounded(px(2.))
                            .bg(p.accent),
                    ),
            )
            .children(SECTIONS.into_iter().enumerate().map(|(index, section)| {
                let current = index == selected;
                gpui_kit::base::Button::new(SharedString::from(format!("settings-nav-{section}")))
                    .relative()
                    .h(px(36.))
                    .pl(px(14.))
                    .pr(px(12.))
                    .gap(px(10.))
                    .justify_start()
                    .rounded(px(6.))
                    .text_color(if current { p.ink } else { p.muted })
                    .when(!current, |b| {
                        b.hover(move |s| s.bg(p.hover).text_color(p.ink))
                    })
                    .role(Role::Tab)
                    .accessibility_label(label(&format!("settings.sections.{section}")))
                    .child(theme::glyph(section_icon(section)))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .child(label(&format!("settings.sections.{section}"))),
                    )
                    .when(section == "update" && pending_update, |b| {
                        b.child(div().ml_auto().size(px(6.)).rounded_full().bg(p.accent))
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.jump_to_section(index, cx);
                        cx.notify();
                    }))
            }));
        let measured = self.ui.settings_width.clone();
        let owner = cx.entity().downgrade();
        div()
            .size_full()
            .min_h_0()
            .px(px(12.))
            .child(
                div()
                    .relative()
                    .size_full()
                    .on_prepaint(move |bounds, window, cx| {
                        if measured.get() != bounds.size.width {
                            measured.set(bounds.size.width);
                            window.defer(cx, move |_, cx| {
                                let _ = owner.update(cx, |_, cx| cx.notify());
                            });
                        }
                    })
                    .child(body)
                    .when(!narrow, |d| d.child(rail)),
            )
            .into_any_element()
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
                cards.push(self.setting_text(
                    "language",
                    "GlobeIcon",
                    "settings.language.title",
                    "settings.language.desc",
                    vec![self.language_select("settings-language", cx)],
                ));
                let themes = ["system", "light", "dark"]
                    .map(|value| (value, label(&format!("settings.theme.{value}"))));
                cards.push(self.setting_text(
                    "theme",
                    "ThemeIcon",
                    "settings.theme.title",
                    "settings.theme.desc",
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
            }
            "startup" => {
                let autostart = self.setting_toggle(
                    "autostart",
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
                let tray = self.setting_toggle(
                    "closeToTray",
                    boolean(settings, "closeToTray"),
                    "set_close_to_tray",
                    "value",
                    window,
                    cx,
                );
                cards.push(self.setting_text(
                    "closeToTray",
                    "TrayIcon",
                    "settings.startup.closeToTray.title",
                    "settings.startup.closeToTray.desc",
                    vec![tray],
                ));
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
                cards.push(self.setting_text(
                    "shortcuts",
                    "KeyboardIcon",
                    "settings.shortcuts.title",
                    "settings.shortcuts.desc",
                    vec![view],
                ));
            }
            "privilege" => {
                cards.push(self.administrator_card(false, window, cx));
                cards.push(self.service_card(cx));
                cards.push(self.sensors_card(settings, window, cx));
                cards.push(self.task_manager_card(cx));
            }
            "motion" => {
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
            }
            "data" => self.data_cards(settings, &mut cards, window, cx),
            "rules" => self.rule_cards(settings, &mut cards, window, cx),
            "update" => self.update_cards(settings, &mut cards, window, cx),
            "about" => self.about_cards(&mut cards, cx),
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
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(p.ink)
                    .child(label(&format!("settings.sections.{section}"))),
            )
            .children(cards)
            .into_any_element()
    }

    // ---- Privileges -----------------------------------------------------------

    fn service_card(&self, cx: &mut Context<Self>) -> AnyElement {
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
            .or(outdated);
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
            if text(&status, "state") == "notInstalled" {
                controls.push(action(
                    self,
                    "install",
                    "settings.privilege.install.action",
                    "install",
                    cx,
                ));
            } else {
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
            label("settings.privilege.statusDesc").into_any_element(),
            error,
            controls,
            None,
        )
    }

    /// Installs, starts, updates or removes the service; only installing and
    /// removing ask first.
    pub(in crate::frontend) fn manage_service(
        &mut self,
        action: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.ui.busy.contains("privilege") {
            return;
        }
        let installing = action == "install";
        let asks = installing || action == "uninstall";
        if !asks {
            self.service_action(action, window, cx);
            return;
        }
        let run = move |this: &mut Desktop, window: &mut Window, cx: &mut Context<Desktop>| {
            this.service_action(action.clone(), window, cx)
        };
        let prefix = if installing {
            "settings.privilege.install"
        } else {
            "settings.privilege.uninstall"
        };
        self.ask(
            label(&format!("{prefix}.title")),
            label(&format!("{prefix}.body")),
            label(if installing {
                "settings.privilege.install.action"
            } else {
                "settings.privilege.uninstall.confirm"
            }),
            !installing,
            run,
            window,
            cx,
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

    fn sensors_card(
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

    fn task_manager_card(&self, cx: &mut Context<Self>) -> AnyElement {
        let status = self.data.get("taskmgr").cloned().unwrap_or(Value::Null);
        let busy = self.ui.busy.contains("taskmgr");
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
            .inert(busy || checking)
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
                .inert(busy || checking)
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
                .inert(busy || checking)
                .on_click(cx.listener(|this, _, window, cx| {
                    this.manage_task_manager("restore".into(), window, cx)
                }))
                .into_any_element(),
            );
        }
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
        if self.ui.busy.contains("taskmgr") || self.options.fixture.is_some() {
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
    /// (Settings.tsx, AdministratorSettings); the onboarding shows the same
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
            Some(label("settings.administrator.scope")),
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

    // ---- Data -------------------------------------------------------------------

    fn data_cards(
        &mut self,
        settings: &Value,
        cards: &mut Vec<AnyElement>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let intervals = [500u64, 1000, 2000, 5000];
        let current = self.interval;
        let interval = self.select(
            "sample-interval",
            label("shell.interval.label"),
            intervals
                .iter()
                .map(|ms| tr!("shell.interval.option", seconds = *ms as f64 / 1000.))
                .collect(),
            intervals.iter().position(|ms| *ms == current),
            false,
            move |this, index, window, cx| this.act(Action::Interval(intervals[index]), window, cx),
            cx,
        );
        cards.push(self.setting_text(
            "interval",
            "TimerIcon",
            "shell.interval.label",
            "settings.data.intervalDesc",
            vec![interval],
        ));
        for (away, glyph) in [("minimized", "MinimizeIcon"), ("tray", "TrayIcon")] {
            let select = self.background_select("settings", away, cx);
            cards.push(self.setting_text(
                if away == "minimized" {
                    "minimizedInterval"
                } else {
                    "trayInterval"
                },
                glyph,
                &format!("settings.data.{away}.title"),
                &format!("settings.data.{away}.desc"),
                vec![select],
            ));
        }
        let unit = self.prefs.get::<String>("netUnit", "bytes".into());
        let units = self.segments(
            "setting-netUnit",
            vec![
                (
                    "MB/s".into(),
                    None,
                    Action::Preference("netUnit".into(), json!("bytes")),
                    unit != "bits",
                ),
                (
                    "Mbps".into(),
                    None,
                    Action::Preference("netUnit".into(), json!("bits")),
                    unit == "bits",
                ),
            ],
            cx,
        );
        cards.push(self.setting_text(
            "netUnit",
            "TransferIcon",
            "settings.data.netUnit.title",
            "settings.data.netUnit.desc",
            vec![units],
        ));
        let history = boolean(settings, "history");
        let toggle = self.setting_toggle(
            "history",
            history,
            "set_history_enabled",
            "enabled",
            window,
            cx,
        );
        cards.push(self.setting_text(
            "history",
            "HistoryIcon",
            "settings.data.history.title",
            if history {
                "settings.data.history.on"
            } else {
                "settings.data.history.off"
            },
            vec![toggle],
        ));
        let usage = self.data.get("historyUsage").cloned();
        let bytes = usage.as_ref().map(|u| number(u, "bytes")).unwrap_or(0.);
        let usage_text = match &usage {
            None => label("settings.data.history.counting"),
            Some(_) if bytes <= 0. => label("settings.data.history.empty"),
            Some(usage) => {
                let size = theme::bytes(bytes, false);
                match usage["first"].as_i64().filter(|t| *t > 0) {
                    Some(first) => tr!(
                        "settings.data.history.sizeSince",
                        size = size,
                        date = short_time(first)
                    ),
                    None => tr!("settings.data.history.size", size = size),
                }
            }
        };
        let clearing = self.ui.busy.contains("clearHistory");
        let clear = self
            .btn(
                "clear-history",
                label("settings.data.history.clear"),
                Some("TrashIcon"),
                Tone::Plain,
            )
            .inert(clearing || bytes <= 0.)
            .on_click(cx.listener(|this, _, window, cx| this.clear_history(window, cx)))
            .into_any_element();
        cards.push(self.setting(
            "savedHistory",
            self.glyph("StorageIcon"),
            label("settings.data.history.saved"),
            Some(usage_text.into_any_element()),
            None,
            vec![clear],
        ));
    }

    pub(in crate::frontend) fn clear_history(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let size = theme::bytes(
            self.data
                .get("historyUsage")
                .map(|v| number(v, "bytes"))
                .unwrap_or(0.),
            false,
        );
        self.ask(
            label("settings.data.history.confirm.title"),
            tr!("settings.data.history.confirm.body", size = size),
            label("settings.data.history.clear"),
            true,
            |this, window, cx| {
                if this.options.fixture.is_some() {
                    return;
                }
                this.ui.busy.insert("clearHistory".into());
                this.request("clear_history", json!({}), None, window, cx);
            },
            window,
            cx,
        );
    }

    // ---- Appearance ---------------------------------------------------------------

    /// Interface scale or font size (Settings.tsx): a short number box with its
    /// unit (`.appearance-number`) and a button back to the default. The box
    /// commits on Enter, when it loses focus and when its spin button is let
    /// go; the arrow keys step it and commit.
    #[allow(clippy::too_many_arguments)]
    fn appearance_setting(
        &mut self,
        setting: &'static appearance::Setting,
        id: &str,
        glyph: &str,
        title: &str,
        desc: &str,
        unit: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.palette;
        let value = self.appearance_value(setting);
        let input = self.appearance_input(setting, window, cx);
        let focused = input.read(cx).focus_handle(cx).is_focused(window);
        let held = self
            .ui
            .spin
            .as_ref()
            .is_some_and(|spin| std::ptr::eq(spin.setting, setting));
        let group = SharedString::from(format!("setting-{id}-number"));
        let number = div()
            .flex()
            .items_center()
            .gap(px(6.))
            .child(
                // `width: 4.5em; padding: 5px 7px` with a 1 px stroke; its
                // height is the 1.4 line height of the text plus those.
                div()
                    .id(SharedString::from(format!("setting-{id}-input")))
                    .group(group.clone())
                    .relative()
                    .w(theme::font(13. * 4.5))
                    .h(theme::font(13.) * 1.4 + px(12.))
                    .px(px(7.))
                    .flex()
                    .items_center()
                    .border_1()
                    .border_color(p.stroke)
                    .rounded(px(4.))
                    .bg(p.control_face)
                    .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                        let up = match event.keystroke.key.as_str() {
                            "up" => true,
                            "down" => false,
                            _ => return,
                        };
                        cx.stop_propagation();
                        let Some(input) = this.ui.inputs.get(setting.key).cloned() else {
                            return;
                        };
                        if let Some(next) = setting.spin(&input.read(cx).value(), up) {
                            this.commit_appearance(setting, next, window, cx);
                        }
                    }))
                    .child(
                        // The input keeps its own 32 px height: centred, its
                        // line sits where the box's text did.
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .flex()
                            .items_center()
                            .child(
                                Input::new(&input)
                                    .appearance(false)
                                    .px_0()
                                    .py_0()
                                    .text_size(theme::font(13.)),
                            ),
                    )
                    .child(self.spin_button(setting, id, &group, focused || held, window, cx))
                    // `:focus-visible`: the 2 px ring in the text colour.
                    .when(focused, |d| d.child(widgets::focus_ring(p.ink, px(1.)))),
            )
            .child(div().text_color(p.ink).child(unit))
            .into_any_element();
        let default = setting.default;
        let reset = self
            .btn(
                format!("setting-{id}-reset"),
                label("settings.appearance.reset"),
                None,
                Tone::Plain,
            )
            .inert(value == default)
            .on_click(cx.listener(move |this, _, window, cx| {
                this.commit_appearance(setting, default, window, cx);
            }))
            .into_any_element();
        self.setting_text(id, glyph, title, desc, vec![number, reset])
    }

    fn appearance_value(&self, setting: &appearance::Setting) -> u32 {
        if setting.key == appearance::SCALE.key {
            self.ui_scale
        } else {
            self.font_size
        }
    }

    /// The number box of an appearance setting, kept across frames.
    fn appearance_input(
        &mut self,
        setting: &'static appearance::Setting,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        if let Some(input) = self.ui.inputs.get(setting.key).cloned() {
            return input;
        }
        let value = self.appearance_value(setting).to_string();
        let input = cx.new(|cx| {
            let mut state = InputState::new(window, cx);
            state.set_value(value, window, cx);
            state
        });
        self.subscriptions.push(cx.subscribe_in(
            &input,
            window,
            move |this, input, event, window, cx| match event {
                InputEvent::Change => {
                    // What a number box accepts: digits, a sign, a point, an exponent.
                    let text = input.read(cx).value();
                    let kept = text
                        .chars()
                        .filter(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E'))
                        .collect::<String>();
                    if kept != text.as_ref() {
                        input.update(cx, |state, cx| state.set_value(kept, window, cx));
                    }
                }
                InputEvent::PressEnter { .. } | InputEvent::Blur => {
                    let next = setting.parse(&input.read(cx).value());
                    this.commit_appearance(setting, next, window, cx);
                }
                _ => {}
            },
        ));
        self.ui.inputs.insert(setting.key.into(), input.clone());
        input
    }

    /// Applies and saves an appearance value, and shows it in its box as
    /// committed (text that is not a number shows the default again).
    fn commit_appearance(
        &mut self,
        setting: &'static appearance::Setting,
        value: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if value != self.appearance_value(setting) {
            self.preference(setting.key, json!(value), window, cx);
        }
        if let Some(input) = self.ui.inputs.get(setting.key).cloned() {
            let text = value.to_string();
            if input.read(cx).value().as_ref() != text {
                input.update(cx, |state, cx| state.set_value(text, window, cx));
            }
        }
    }

    /// A number box's spin button (`::-webkit-inner-spin-button`, drawn by
    /// Chromium): 15 px wide at the end of the box's text, its two halves
    /// stepping up and down. It shows while the box is pointed at or focused;
    /// the arrow under the pointer lights up. Held, it steps once, again after
    /// 250 ms and then every 50 ms the way the half under the pointer points,
    /// as Chromium repeats it; the box commits when it is let go.
    fn spin_button(
        &self,
        setting: &'static appearance::Setting,
        id: &str,
        group: &SharedString,
        shown: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let dark = self.palette.dark;
        let scale = window.scale_factor();
        let half = |up: bool| {
            let lit = self.ui.spin_hover == Some((setting.key, up));
            let color = theme::token(
                if lit {
                    "spin-arrow-hover"
                } else {
                    "spin-arrow"
                },
                dark,
            );
            let arrow = assets::spin_arrow(up, color, scale, cx);
            div()
                .id(SharedString::from(format!(
                    "setting-{id}-{}",
                    if up { "up" } else { "down" }
                )))
                .relative()
                .flex_1()
                .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                    let here = Some((setting.key, up));
                    if *hovered {
                        this.ui.spin_hover = here;
                    } else if this.ui.spin_hover == here {
                        this.ui.spin_hover = None;
                    }
                    cx.notify();
                }))
                .child(
                    // 15 × 9 in the middle of the half, on whole device pixels,
                    // area-filtered from its double-resolution raster.
                    canvas(
                        |_, _, _| {},
                        move |bounds, _, window, _| {
                            let Some(arrow) = arrow else {
                                return;
                            };
                            let s = window.scale_factor();
                            let snap = |v: Pixels| px((f32::from(v) * s).round() / s);
                            let size = size(px(15.), px(9.));
                            let origin = bounds.center() - point(size.width / 2., size.height / 2.);
                            let _ = window.paint_image_subpixel(
                                Bounds::new(point(snap(origin.x), snap(origin.y)), size),
                                arrow,
                                0,
                            );
                        },
                    )
                    .absolute()
                    .size_full(),
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.spin_press(setting, up, window, cx);
                    }),
                )
        };
        // Wherever the button is let go, and if the press got lost (the
        // pointer moving with no button down), the hold ends.
        let owner = cx.entity().downgrade();
        let track = canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                let held = move |owner: &WeakEntity<Desktop>, cx: &App| {
                    owner.upgrade().filter(|desktop| {
                        desktop
                            .read(cx)
                            .ui
                            .spin
                            .as_ref()
                            .is_some_and(|spin| std::ptr::eq(spin.setting, setting))
                    })
                };
                let moved = owner.clone();
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                    if phase != DispatchPhase::Capture {
                        return;
                    }
                    let Some(desktop) = held(&moved, cx) else {
                        return;
                    };
                    let over = bounds
                        .contains(&event.position)
                        .then(|| event.position.y < bounds.center().y);
                    let pressed = event.pressed_button == Some(MouseButton::Left);
                    desktop.update(cx, |this, cx| this.spin_track(over, pressed, window, cx));
                });
                window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
                    if phase != DispatchPhase::Capture || event.button != MouseButton::Left {
                        return;
                    }
                    if let Some(desktop) = held(&owner, cx) {
                        desktop.update(cx, |this, cx| this.spin_release(window, cx));
                    }
                });
            },
        )
        .absolute()
        .size_full();
        div()
            .relative()
            .flex_shrink_0()
            .w(px(15.))
            .h(theme::font(13.) * 1.4)
            .flex()
            .flex_col()
            .bg(theme::token("spin-face", dark))
            .opacity(if shown { 1. } else { 0. })
            .when(!shown, |d| d.group_hover(group.clone(), |s| s.opacity(1.)))
            .child(half(true))
            .child(half(false))
            .child(track)
    }

    /// The spin button pressed: the box takes focus and steps, and keeps
    /// stepping while it is held.
    fn spin_press(
        &mut self,
        setting: &'static appearance::Setting,
        up: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(input) = self.ui.inputs.get(setting.key).cloned() else {
            return;
        };
        input.update(cx, |state, cx| state.focus(window, cx));
        self.spin_step(setting, up, window, cx);
        let repeat = cx.spawn_in(window, async move |this, cx| {
            // Steps keep to a schedule from the press: a timer fires on the system
            // tick (15.6 ms by default), so waiting the interval after each step
            // would repeat every 62.5 ms instead of every 50 on average.
            let pressed = cx.background_executor().now();
            for step in 0u32.. {
                let due = pressed + scrollbar::REPEAT_DELAY + scrollbar::REPEAT_INTERVAL * step;
                let now = cx.background_executor().now();
                cx.background_executor()
                    .timer(due.saturating_duration_since(now))
                    .await;
                let held = this.update_in(cx, |this, window, cx| {
                    let Some(spin) = this.ui.spin.as_ref() else {
                        return false;
                    };
                    if let Some(up) = spin.over {
                        this.spin_step(setting, up, window, cx);
                    }
                    true
                });
                if !matches!(held, Ok(true)) {
                    break;
                }
            }
        });
        self.ui.spin = Some(widgets::Spin {
            setting,
            over: Some(up),
            repeat,
        });
        cx.notify();
    }

    /// One step of the box's text; it commits when the button is let go.
    fn spin_step(
        &mut self,
        setting: &'static appearance::Setting,
        up: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(input) = self.ui.inputs.get(setting.key).cloned() else {
            return;
        };
        let text = input.read(cx).value();
        if let Some(next) = setting.spin(&text, up).map(|next| next.to_string())
            && next != text.as_ref()
        {
            input.update(cx, |state, cx| state.set_value(next, window, cx));
        }
    }

    /// The pointer moved while the spin button is held.
    fn spin_track(
        &mut self,
        over: Option<bool>,
        pressed: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !pressed {
            self.spin_release(window, cx);
        } else if let Some(spin) = &mut self.ui.spin {
            spin.over = over;
        }
    }

    /// The spin button let go: the box commits what it shows, as the
    /// original's `change` event did on release.
    fn spin_release(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(spin) = self.ui.spin.take() else {
            return;
        };
        drop(spin.repeat);
        if let Some(input) = self.ui.inputs.get(spin.setting.key).cloned() {
            let next = spin.setting.parse(&input.read(cx).value());
            self.commit_appearance(spin.setting, next, window, cx);
        }
        cx.notify();
    }

    // ---- Rules ------------------------------------------------------------------

    /// An inline text box kept across frames; Enter runs `submit`. The card
    /// draws its placeholder (`Desktop::text_field`).
    fn inline_input(
        &mut self,
        key: &str,
        initial: &str,
        submit: fn(&mut Desktop, &mut Window, &mut Context<Desktop>),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        if let Some(input) = self.ui.inputs.get(key).cloned() {
            return input;
        }
        let initial = initial.to_owned();
        let input = cx.new(|cx| {
            let mut state = InputState::new(window, cx);
            state.set_value(initial, window, cx);
            state
        });
        let error_key = key.to_owned();
        self.subscriptions.push(cx.subscribe_in(
            &input,
            window,
            move |this, _, event, window, cx| match event {
                InputEvent::Change => {
                    this.ui.errors.remove(&error_key);
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => submit(this, window, cx),
                _ => {}
            },
        ));
        self.ui.inputs.insert(key.into(), input.clone());
        input
    }

    fn add_rule(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(input) = self.ui.inputs.get("ruleDraft").cloned() else {
            return;
        };
        let name = input.read(cx).value().trim().to_owned();
        if name.is_empty() {
            return;
        }
        let mut rules = values(&self.data["settings"]["rules"]).to_vec();
        if rules
            .iter()
            .any(|r| text(r, "name").eq_ignore_ascii_case(&name))
        {
            self.ui.errors.insert(
                "ruleDraft".into(),
                tr!("settings.rules.exists", name = name),
            );
            cx.notify();
            return;
        }
        let id = format!(
            "native-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        );
        rules.push(json!({"id":id,"name":name,"enabled":true,"priority":null,"efficiency":true}));
        self.request(
            "set_rules",
            json!({"rules":rules}),
            Some("settings"),
            window,
            cx,
        );
        input.update(cx, |state, cx| state.set_value("", window, cx));
        self.ui.errors.remove("ruleDraft");
        self.ui.focus_rule = Some((id, Instant::now()));
        cx.notify();
    }

    fn update_rule(&mut self, id: &str, patch: Value, window: &mut Window, cx: &mut Context<Self>) {
        let mut rules = values(&self.data["settings"]["rules"]).to_vec();
        for rule in &mut rules {
            if rule["id"] == id {
                for (key, value) in patch.as_object().into_iter().flatten() {
                    rule[key] = value.clone();
                }
            }
        }
        self.request(
            "set_rules",
            json!({"rules":rules}),
            Some("settings"),
            window,
            cx,
        );
    }

    fn rule_cards(
        &mut self,
        settings: &Value,
        cards: &mut Vec<AnyElement>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let p = self.palette;
        let input = self.inline_input(
            "ruleDraft",
            "",
            |this, window, cx| this.add_rule(window, cx),
            window,
            cx,
        );
        let draft = input.read(cx).value().trim().to_owned();
        let add = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(div().w(px(220.)).child(self.text_field(
                &input,
                Input::new(&input),
                Field::Boxed,
                label("settings.rules.placeholder").into(),
                true,
                window,
                cx,
            )))
            .child(
                self.btn("rule-add", label("settings.rules.add"), None, Tone::Plain)
                    .inert(draft.is_empty())
                    .on_click(cx.listener(|this, _, window, cx| this.add_rule(window, cx))),
            )
            .into_any_element();
        cards.push(self.setting(
            "rules",
            self.glyph("BoltIcon"),
            label("settings.rules.title"),
            Some(label("settings.rules.desc").into_any_element()),
            self.ui.errors.get("ruleDraft").cloned(),
            vec![add],
        ));
        let stats = self.data.get("ruleStats").cloned().unwrap_or(Value::Null);
        for rule in values(&settings["rules"]).to_vec() {
            let id = text(&rule, "id").to_owned();
            let name = text(&rule, "name").to_owned();
            let enabled = boolean(&rule, "enabled");
            let process = values(&self.snapshot["processes"])
                .iter()
                .find(|q| text(q, "name").eq_ignore_ascii_case(&name))
                .cloned();
            let stat = stats.get(&id).cloned().or_else(|| {
                values(&stats)
                    .iter()
                    .find(|s| text(s, "id") == id || text(s, "name") == name)
                    .cloned()
            });
            let summary = if rule["priority"].is_null() && rule["efficiency"].is_null() {
                label("settings.rules.summary.noAction")
            } else if !enabled {
                label("settings.rules.summary.disabled")
            } else {
                let count = |key: &str| stat.as_ref().and_then(|s| s[key].as_u64()).unwrap_or(0);
                let (applied, denied, failed) =
                    (count("applied"), count("denied"), count("failed"));
                if applied + denied + failed == 0 {
                    label("settings.rules.summary.notSeen")
                } else {
                    let mut parts = vec![];
                    if applied > 0 {
                        parts.push(tr!("settings.rules.summary.applied", count = applied));
                    }
                    if denied > 0 {
                        parts.push(tr!("settings.rules.summary.denied", count = denied));
                    }
                    if failed > 0 {
                        parts.push(tr!("settings.rules.summary.failed", count = failed));
                    }
                    tr!(
                        "settings.rules.summary.sentence",
                        parts = parts.join(&label("settings.rules.summary.separator"))
                    )
                }
            };
            let priority_values = std::iter::once("")
                .chain(RULE_PRIORITIES)
                .collect::<Vec<_>>();
            let current_priority = rule["priority"].as_str().unwrap_or("");
            let priority = {
                let id = id.clone();
                let options = priority_values
                    .iter()
                    .map(|value| {
                        if value.is_empty() {
                            label("settings.rules.priority.keep")
                        } else {
                            tr!(
                                "settings.rules.priority.option",
                                priority = label(&format!("processes.priority.{value}"))
                            )
                        }
                    })
                    .collect();
                self.select(
                    format!("rule-priority-{id}"),
                    tr!("settings.rules.priority.label", name = name),
                    options,
                    priority_values.iter().position(|v| *v == current_priority),
                    false,
                    move |this, index, window, cx| {
                        let value = priority_values[index];
                        let value = if value.is_empty() {
                            Value::Null
                        } else {
                            json!(value)
                        };
                        this.update_rule(&id, json!({"priority":value}), window, cx);
                    },
                    cx,
                )
            };
            let efficiency = {
                let id = id.clone();
                let current = match rule["efficiency"].as_bool() {
                    None => 0,
                    Some(true) => 1,
                    Some(false) => 2,
                };
                self.select(
                    format!("rule-efficiency-{id}"),
                    tr!("settings.rules.efficiency.label", name = name),
                    vec![
                        label("settings.rules.efficiency.keep"),
                        label("settings.rules.efficiency.on"),
                        label("settings.rules.efficiency.off"),
                    ],
                    Some(current),
                    false,
                    move |this, index, window, cx| {
                        let value = match index {
                            1 => json!(true),
                            2 => json!(false),
                            _ => Value::Null,
                        };
                        this.update_rule(&id, json!({"efficiency":value}), window, cx);
                    },
                    cx,
                )
            };
            let toggle = {
                let id = id.clone();
                self.toggle(
                    format!("rule-toggle-{id}"),
                    tr!("settings.rules.toggle", name = name),
                    enabled,
                    false,
                    move |this, on, window, cx| {
                        this.update_rule(&id, json!({"enabled":on}), window, cx)
                    },
                    window,
                    cx,
                )
            };
            let delete = {
                let id = id.clone();
                self.btn_small(
                    format!("rule-delete-{id}"),
                    "",
                    Some("DismissIcon"),
                    Tone::Ghost,
                )
                .accessibility_label(tr!("settings.rules.deleteLabel", name = name))
                .tooltip(|window, cx| {
                    gpui_kit::component::tooltip::Tooltip::new(label("settings.rules.delete"))
                        .build(window, cx)
                })
                .on_click(cx.listener(move |this, _, window, cx| {
                    let mut rules = values(&this.data["settings"]["rules"]).to_vec();
                    rules.retain(|r| r["id"] != id.as_str());
                    this.request(
                        "set_rules",
                        json!({"rules":rules}),
                        Some("settings"),
                        window,
                        cx,
                    );
                }))
                .into_any_element()
            };
            let focused = self
                .ui
                .focus_rule
                .as_ref()
                .is_some_and(|(rule, _)| *rule == id);
            let card = self.setting(
                &format!("rule-{id}"),
                self.process_icon(process.as_ref(), 20., cx),
                div().font_weight(FontWeight::SEMIBOLD).child(name.clone()),
                Some(summary.into_any_element()),
                None,
                vec![priority, efficiency, toggle, delete],
            );
            cards.push(if focused {
                div()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(p.accent)
                    .child(card)
                    .into_any_element()
            } else {
                card
            });
        }
    }

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

    fn update_cards(
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
            version = text(&self.data["info"], "version"),
            channel = label(&format!(
                "updates.dialog.{}",
                text(settings, "updateChannel")
            ))
        );
        let offered = matches!(
            state.as_str(),
            "ready" | "available" | "downloading" | "installing"
        );
        let mut controls = vec![];
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
        if offered && !version.is_empty() {
            cards.push(self.inline_release(&status, &version, window, cx));
        }
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
                        Input::new(&input),
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

    /// The release on offer, inline (Settings.tsx, update-release-card).
    fn inline_release(
        &self,
        status: &Value,
        version: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.palette;
        let current = text(&self.data["info"], "version").to_owned();
        let open_version = version.to_owned();
        div()
            .my(px(4.))
            .border_1()
            .border_color(p.stroke)
            .rounded(px(6.))
            .bg(p.layer)
            .child(
                div()
                    .pt(px(20.))
                    .px(px(24.))
                    .pb(px(18.))
                    .flex()
                    .flex_col()
                    .gap(px(14.))
                    .child(self.update_head(
                        tr!("updates.dialog.installed", version = version),
                        self.update_subtitle(status, version, true),
                    ))
                    .child(self.update_versions(Some(&current), version, false))
                    .child(self.release_reader(status, Some(&current), version, false, window, cx)),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .justify_between()
                    .gap(px(8.))
                    .px(px(20.))
                    .py(px(12.))
                    .border_t_1()
                    .border_color(p.stroke)
                    .child(
                        self.btn(
                            "inline-github",
                            label("updates.dialog.github"),
                            Some("ExternalIcon"),
                            Tone::Ghost,
                        )
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.request(
                                    "open_release_page",
                                    json!({"version":open_version}),
                                    None,
                                    window,
                                    cx,
                                );
                            },
                        )),
                    )
                    .child(
                        self.btn(
                            "inline-dialog",
                            label("updates.openDialog"),
                            None,
                            Tone::Plain,
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.open_dialog("update", json!({}), window, cx)
                        })),
                    ),
            )
            .into_any_element()
    }

    // ---- About ------------------------------------------------------------------

    fn about_cards(&mut self, cards: &mut Vec<AnyElement>, cx: &mut Context<Self>) {
        let p = self.palette;
        let manifest = argus_app::config::manifest();
        let privilege = self.data.get("privilege").cloned().unwrap_or(Value::Null);
        let group = text(&manifest["community"], "group").to_owned();
        let homepage = text(manifest, "homepage")
            .trim_start_matches("https://")
            .to_owned();
        cards.push(
            div()
                .id("setting-about")
                .flex()
                .items_center()
                .gap(px(16.))
                .min_h(px(92.))
                .px(px(16.))
                .py(px(12.))
                .border_1()
                .border_color(p.stroke)
                .rounded(px(6.))
                .bg(p.layer)
                .child(img("argus/logo.svg").size(px(48.)).flex_shrink_0())
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .flex()
                                .items_baseline()
                                .gap(px(8.))
                                .child(
                                    div()
                                        .font_family(theme::DISPLAY_FONT)
                                        .text_size(theme::font(18.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(argus_app::config::product_name()),
                                )
                                .child(
                                    div()
                                        .text_size(theme::font(13.))
                                        .text_color(p.muted)
                                        .child(text(&self.data["info"], "version").to_owned()),
                                ),
                        )
                        .child(
                            div()
                                .mt(px(2.))
                                .text_size(theme::font(12.))
                                .text_color(p.muted)
                                .child(label("settings.about.tagline")),
                        )
                        .child(
                            div()
                                .mt(px(2.))
                                .text_size(theme::font(12.))
                                .text_color(p.muted)
                                .child(label(if boolean(&self.data["info"], "elevated") {
                                    "settings.about.admin"
                                } else if service_connected(&privilege) {
                                    "settings.about.service"
                                } else {
                                    "settings.about.standard"
                                })),
                        )
                        .child(
                            div()
                                .mt(px(2.))
                                .text_size(theme::font(12.))
                                .text_color(p.muted)
                                .child(format!("© 2026 {}", env!("CARGO_PKG_AUTHORS"))),
                        ),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .flex()
                        .gap(px(8.))
                        .child(
                            self.btn(
                                "about-whats-new",
                                label("settings.about.whatsNew"),
                                None,
                                Tone::Plain,
                            )
                            .on_click(cx.listener(
                                |this, _, window, cx| {
                                    this.open_dialog("whatsnew", json!({}), window, cx)
                                },
                            )),
                        )
                        .child(
                            self.btn(
                                "about-quit",
                                label("settings.about.quit"),
                                None,
                                Tone::Plain,
                            )
                            .on_click(cx.listener(
                                |this, _, window, cx| {
                                    this.request("quit", json!({}), None, window, cx)
                                },
                            )),
                        ),
                )
                .into_any_element(),
        );
        let homepage_desc = {
            // The address reads in the ink colour inside the description.
            let marked = tr!("settings.about.homepage.desc", url = "");
            let start = marked.find('').unwrap_or(marked.len());
            let full = marked.replacen('', &homepage, 1);
            let end = (start + homepage.len()).min(full.len());
            StyledText::new(full)
                .with_highlights(vec![(
                    start..end,
                    HighlightStyle {
                        color: Some(p.ink),
                        ..Default::default()
                    },
                )])
                .into_any_element()
        };
        cards.push(self.setting(
            "homepage",
            self.glyph("StarIcon"),
            label("settings.about.homepage.title"),
            Some(homepage_desc),
            None,
            vec![
                    self.btn(
                        "homepage",
                        label("settings.about.homepage.open"),
                        Some("ExternalIcon"),
                        Tone::Accent,
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.request("open_homepage", json!({}), None, window, cx)
                    }))
                    .into_any_element(),
                ],
        ));
        cards.push(self.setting_text(
            "onboarding",
            "SparkleIcon",
            "settings.about.onboarding.title",
            "settings.about.onboarding.desc",
            vec![
                self.btn("onboarding", label("settings.about.onboarding.open"), None, Tone::Plain)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.open_dialog("onboarding", json!({"step":0}), window, cx)
                    }))
                    .into_any_element(),
            ],
        ));
        let copy_group = group.clone();
        cards.push(self.setting(
            "community",
            self.glyph("ChatIcon"),
            label("settings.about.community.title"),
            Some(tr!("settings.about.community.desc", group = group).into_any_element()),
            None,
            vec![
                    self.btn(
                        "copy-community",
                        label("settings.about.community.copy"),
                        None,
                        Tone::Plain,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(copy_group.clone()));
                        this.notify_toast_for(
                            ToastKind::Success,
                            tr!(
                                "actions.copied",
                                what = label("settings.about.community.number")
                            ),
                            1800,
                        );
                        cx.notify();
                    }))
                    .into_any_element(),
                    self.btn(
                        "community",
                        label("settings.about.community.join"),
                        None,
                        Tone::Accent,
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.request("open_community", json!({}), None, window, cx)
                    }))
                    .into_any_element(),
                ],
        ));
        for (id, key) in [
            ("license", "settings.about.license.view"),
            ("notices", "settings.about.notices.view"),
        ] {
            cards.push(self.setting_text(
                id,
                "DocumentIcon",
                &format!("settings.about.{id}.title"),
                &format!("settings.about.{id}.desc"),
                vec![
                        self.btn(id, label(key), None, Tone::Plain)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.open_dialog(id, json!({}), window, cx)
                            }))
                            .into_any_element(),
                    ],
            ));
        }
        cards.push(self.setting_text(
            "log",
            "StorageIcon",
            "settings.about.log.title",
            "settings.about.log.desc",
            vec![
                    self.btn("log", label("settings.about.log.open"), None, Tone::Plain)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.request("open_log_dir", json!({}), None, window, cx)
                        }))
                        .into_any_element(),
                ],
        ));
    }

    // ---- Hotkey card ------------------------------------------------------------

    /// The show/hide hotkey: the combination and a switch on top; below,
    /// while recording, confirming or refusing, what is going on.
    fn hotkey_card(
        &self,
        settings: &Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.palette;
        let card = &self.ui.hotkey;
        let mode = card.mode;
        let hotkey = text(settings, "hotkey").to_owned();
        let enabled = boolean(settings, "hotkeyEnabled");
        let flash = card
            .flash
            .map(|at| 1. - (at.elapsed().as_secs_f32() / 1.2).min(1.))
            .unwrap_or(0.);
        if flash > 0. {
            window.request_animation_frame();
        }
        let error_text = match &card.message {
            None if mode == HotkeyMode::Idle && card.done.is_none() => {
                hotkey_error_text(&settings["hotkeyError"])
            }
            None => None,
            Some(HotkeyMessage::Error(error)) => hotkey_error_text(error),
            Some(HotkeyMessage::Issue(issue, shortcut)) => Some(issue_text(issue, shortcut)),
            Some(HotkeyMessage::Hint(hint)) => match hint {
                recorder::Hint::NoKey => Some(label("settings.hotkey.issue.noKey")),
                recorder::Hint::Order => Some(label("settings.hotkey.hint.order")),
                recorder::Hint::MultipleKeys => Some(label("settings.hotkey.hint.multipleKeys")),
                recorder::Hint::Unsupported(key) => Some(tr!(
                    "settings.hotkey.issue.unsupported",
                    key = if key.is_empty() {
                        label("settings.hotkey.unknownKey")
                    } else {
                        key.clone()
                    }
                )),
                recorder::Hint::LockKey => None,
            },
            _ => None,
        };
        let note = match &card.message {
            Some(HotkeyMessage::MaybeTaken) => Some(label("settings.hotkey.hint.maybeTaken")),
            Some(HotkeyMessage::Hint(recorder::Hint::LockKey)) => {
                Some(label("settings.hotkey.hint.lockKey"))
            }
            _ if mode == HotkeyMode::Recording && error_text.is_none() => {
                Some(label("settings.hotkey.help"))
            }
            _ => None,
        };
        let warning = match &card.message {
            Some(HotkeyMessage::Warning(warnings, shortcut)) => Some(
                warnings
                    .iter()
                    .map(|w| {
                        argus_core::i18n::format(
                            &format!("settings.hotkey.warning.{w}"),
                            &[("shortcut", &recorder::shortcut_text(shortcut))],
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(" "),
            ),
            _ => None,
        };
        let confirming =
            mode == HotkeyMode::Confirm || (mode == HotkeyMode::Saving && warning.is_some());
        let failed =
            enabled && matches!(text(&settings["hotkeyError"], "kind"), "taken" | "failed");
        let open = mode != HotkeyMode::Idle || card.done.is_some() || error_text.is_some();
        let idle = mode == HotkeyMode::Idle;
        let head = div()
            .flex()
            .items_center()
            .gap(px(16.))
            .px(px(16.))
            .py(px(14.))
            .child(
                div()
                    .w(px(20.))
                    .flex_shrink_0()
                    .child(self.glyph("KeyboardIcon")),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(label("settings.hotkey.title"))
                    .child(
                        div()
                            .mt(px(2.))
                            .text_size(theme::font(12.))
                            .text_color(p.muted)
                            .child(label("settings.hotkey.desc")),
                    ),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(self.keycaps(&recorder::shortcut_keys(&hotkey), !enabled, flash))
                    .child(
                        self.btn(
                            "hotkey-change",
                            label("settings.hotkey.change"),
                            None,
                            Tone::Plain,
                        )
                        .track_focus(&self.focus_for("hotkey-change", cx))
                        .inert_dimmed(!idle)
                        .on_click(cx.listener(|this, _, window, cx| this.begin_hotkey(window, cx))),
                    )
                    .child(self.toggle(
                        "hotkey-enabled",
                        label("settings.hotkey.title"),
                        enabled,
                        !idle || card.switching,
                        |this, on, window, cx| this.set_hotkey_enabled(on, window, cx),
                        window,
                        cx,
                    )),
            );
        let mut note_column = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .items_start()
            .gap(px(6.))
            .py(px(4.))
            .text_size(theme::font(12.))
            .text_color(p.muted);
        if let Some((off, saved, _)) = &card.done {
            let line = div()
                .flex()
                .items_center()
                .gap(px(8.))
                .text_color(p.ink)
                .child(icon("CheckIcon", p.accent).size(px(14.)));
            note_column = note_column.child(if *off {
                line.child(label("settings.hotkey.done.off"))
            } else {
                line.child(self.rich(
                    tr!("settings.hotkey.done.saved", keys = ""),
                    vec![(
                        '',
                        self.keycaps(&recorder::shortcut_keys(saved), false, 0.),
                    )],
                ))
            });
        }
        if !idle {
            let keys = if mode == HotkeyMode::Recording {
                if card.recording.chord.is_empty() {
                    div()
                        .px(px(8.))
                        .py(px(2.))
                        .border_1()
                        .border_b_2()
                        .border_dashed()
                        .border_color(p.stroke_strong)
                        .rounded(px(4.))
                        .bg(p.face)
                        .text_color(p.muted)
                        .child(label("settings.hotkey.recording"))
                        .into_any_element()
                } else {
                    let parts = card
                        .recording
                        .chord
                        .iter()
                        .map(|key| match key {
                            recorder::ChordKey::Modifier(name) => (*name).to_owned(),
                            recorder::ChordKey::Lock => "Caps Lock".to_owned(),
                            recorder::ChordKey::Key(name) => name.clone(),
                        })
                        .collect::<Vec<_>>();
                    self.keycaps(&parts, false, 0.)
                }
            } else {
                self.keycaps(&recorder::shortcut_keys(&card.candidate), false, 0.)
            };
            note_column = note_column.child(keys);
        }
        if let Some(warning) = warning {
            note_column = note_column.child(
                div()
                    .flex()
                    .gap(px(6.))
                    .text_color(p.ink)
                    .child(
                        div()
                            .mt(px(2.))
                            .child(icon("WarningIcon", p.io).size(px(14.))),
                    )
                    .child(warning),
            );
        }
        if let Some(error) = error_text {
            note_column = note_column.child(div().text_color(p.danger).child(error));
        }
        if let Some(note) = note {
            note_column = note_column.child(note);
        }
        let mut controls = div().flex_shrink_0().flex().items_center().gap(px(8.));
        if confirming {
            controls = controls
                .child(
                    self.btn(
                        "hotkey-rerecord",
                        label("settings.hotkey.rerecord"),
                        None,
                        Tone::Plain,
                    )
                    .track_focus(&self.focus_for("hotkey-again", cx))
                    .inert(mode == HotkeyMode::Saving)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.ui.hotkey.message = None;
                        this.listen_hotkey(None, window, cx);
                    })),
                )
                .child(
                    self.btn(
                        "hotkey-use",
                        label("settings.hotkey.confirmUse"),
                        None,
                        Tone::Accent,
                    )
                    .inert(mode == HotkeyMode::Saving)
                    .on_click(cx.listener(|this, _, window, cx| {
                        let candidate = this.ui.hotkey.candidate.clone();
                        this.save_hotkey(candidate, window, cx);
                    })),
                );
        }
        if mode == HotkeyMode::Recording {
            let default = text(settings, "defaultHotkey").to_owned();
            if !default.is_empty() && hotkey != default {
                controls = controls.child(
                    self.btn(
                        "hotkey-reset",
                        label("settings.hotkey.reset"),
                        None,
                        Tone::Plain,
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.save_hotkey(default.clone(), window, cx)
                    })),
                );
            }
            controls = controls.child(
                self.btn("hotkey-cancel", label("common.cancel"), None, Tone::Plain)
                    .track_focus(&self.focus_for("hotkey-cancel", cx))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.finish_hotkey(true, false, window, cx)
                    })),
            );
        }
        if idle && failed {
            controls = controls.child(
                self.btn(
                    "hotkey-retry",
                    label("settings.hotkey.retry"),
                    None,
                    Tone::Plain,
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.ui.hotkey.done = None;
                    this.ui.hotkey.message = None;
                    this.request("resume_hotkey", json!({}), Some("settings"), window, cx);
                })),
            );
        }
        let session = card.session;
        let drawer = open.then(|| {
            div()
                .flex()
                .items_center()
                .gap(px(16.))
                .min_h(px(52.))
                .pl(px(52.))
                .pr(px(16.))
                .py(px(8.))
                .border_t_1()
                .border_color(p.stroke)
                .child(note_column)
                .child(controls)
                .with_animation(
                    ("hotkey-drawer", session as usize),
                    Animation::new(Duration::from_millis(if cx.reduce_motion() {
                        1
                    } else {
                        250
                    }))
                    .with_easing(theme::ease_out),
                    |drawer, delta| drawer.opacity(delta).mt(px((1. - delta) * -6.)),
                )
        });
        let tint = self.located_tint("hotkey");
        let (border, fill) = self.located_colors(tint);
        let card = div()
            .id("setting-hotkey")
            .flex()
            .flex_col()
            .border_1()
            .border_color(border)
            .rounded(px(6.))
            .bg(fill)
            .overflow_hidden()
            .child(head)
            .children(drawer)
            .when(!idle, |card| {
                card.capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                    this.hotkey_key(event, true, window, cx);
                }))
                .capture_key_up(cx.listener(|this, event: &KeyUpEvent, window, cx| {
                    let down = KeyDownEvent {
                        keystroke: event.keystroke.clone(),
                        is_held: false,
                        prefer_character_input: false,
                    };
                    this.hotkey_key(&down, false, window, cx);
                }))
                .on_modifiers_changed(cx.listener(
                    |this, event: &ModifiersChangedEvent, window, cx| {
                        this.hotkey_modifiers(event, window, cx);
                    },
                ))
                .on_mouse_down_out(cx.listener(|this, _, window, cx| {
                    if this.ui.hotkey.mode != HotkeyMode::Idle {
                        this.finish_hotkey(true, false, window, cx);
                    }
                }))
            });
        // The card clips its drawer, so its ring sits on a wrapper.
        match self.located_ring(tint) {
            Some(ring) => div()
                .relative()
                .flex()
                .flex_col()
                .child(card)
                .child(ring)
                .into_any_element(),
            None => card.into_any_element(),
        }
    }

    pub(in crate::frontend) fn begin_hotkey(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.ui.hotkey.mode != HotkeyMode::Idle {
            return;
        }
        self.ui.hotkey.session += 1;
        self.ui.hotkey.done = None;
        self.ui.hotkey.message = None;
        self.listen_hotkey(None, window, cx);
    }

    /// Starts or restarts listening for keys, keeping held modifiers if given.
    fn listen_hotkey(
        &mut self,
        from: Option<recorder::Recording>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Each start renews the host's wait for the hotkey to come back.
        self.request("suspend_hotkey", json!({}), None, window, cx);
        let card = &mut self.ui.hotkey;
        card.recording = from.unwrap_or_default();
        card.candidate.clear();
        card.mode = HotkeyMode::Recording;
        card.last_key = Some(Instant::now());
        card.quiet_since = None;
        self.focus_for("hotkey-cancel", cx).focus(window, cx);
        cx.notify();
    }

    /// Ends the recording; `resume` brings the old hotkey back.
    pub(in crate::frontend) fn finish_hotkey(
        &mut self,
        resume: bool,
        keep_message: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let card = &mut self.ui.hotkey;
        card.session += 1;
        card.mode = HotkeyMode::Idle;
        card.recording = recorder::Recording::default();
        card.candidate.clear();
        card.last_key = None;
        card.quiet_since = None;
        if !keep_message {
            card.message = None;
        }
        if resume {
            self.request("resume_hotkey", json!({}), Some("settings"), window, cx);
        }
        self.focus_for("hotkey-change", cx).focus(window, cx);
        cx.notify();
    }

    fn succeed_hotkey(
        &mut self,
        off: bool,
        hotkey: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let card = &mut self.ui.hotkey;
        card.session += 1;
        card.mode = HotkeyMode::Idle;
        card.recording = recorder::Recording::default();
        card.candidate.clear();
        card.message = None;
        card.last_key = None;
        card.done = Some((off, hotkey, Instant::now()));
        if !off {
            card.flash = Some(Instant::now());
        }
        self.focus_for("hotkey-change", cx).focus(window, cx);
        cx.notify();
    }

    fn save_hotkey(&mut self, hotkey: String, window: &mut Window, cx: &mut Context<Self>) {
        let card = &mut self.ui.hotkey;
        card.candidate = hotkey.clone();
        card.mode = HotkeyMode::Saving;
        self.request(
            "set_hotkey",
            json!({"hotkey":hotkey}),
            Some("settings"),
            window,
            cx,
        );
        cx.notify();
    }

    fn set_hotkey_enabled(&mut self, on: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.ui.hotkey.done = None;
        self.ui.hotkey.message = None;
        self.ui.hotkey.switching = true;
        self.request(
            "set_hotkey_enabled",
            json!({"enabled":on}),
            Some("settings"),
            window,
            cx,
        );
    }

    fn attempt_hotkey(&mut self, hotkey: String, window: &mut Window, cx: &mut Context<Self>) {
        match recorder::judge(&hotkey) {
            Verdict::Block(issue) => {
                self.ui.hotkey.message = Some(HotkeyMessage::Issue(issue, hotkey));
            }
            Verdict::Warn(warnings) => {
                let card = &mut self.ui.hotkey;
                card.candidate = hotkey.clone();
                card.message = Some(HotkeyMessage::Warning(warnings, hotkey));
                card.mode = HotkeyMode::Confirm;
                card.last_key = Some(Instant::now());
                self.request("suspend_hotkey", json!({}), None, window, cx);
                self.focus_for("hotkey-again", cx).focus(window, cx);
            }
            Verdict::Ok => self.save_hotkey(hotkey, window, cx),
        }
        cx.notify();
    }

    fn hotkey_key(
        &mut self,
        event: &KeyDownEvent,
        down: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mode = self.ui.hotkey.mode;
        if mode == HotkeyMode::Confirm {
            if down && event.keystroke.key == "escape" {
                cx.stop_propagation();
                self.finish_hotkey(true, false, window, cx);
            }
            return;
        }
        if mode != HotkeyMode::Recording {
            return;
        }
        let m = event.keystroke.modifiers;
        let mods = Mods {
            ctrl: m.control,
            alt: m.alt,
            shift: m.shift,
            win: m.platform,
        };
        let key = event.keystroke.key.clone();
        let card = &mut self.ui.hotkey;
        let before_held = card.recording.mods != Mods::default() || !card.recording.keys.is_empty();
        let before_hint = card.recording.hint.clone();
        let effect = if down {
            card.recording.key_down(&key, mods, event.is_held)
        } else {
            card.recording.key_up(&key)
        };
        // Tab still moves the focus as it ends the recording.
        if !(effect == Effect::Cancel && key == "tab") {
            cx.stop_propagation();
        }
        if down && !before_held {
            card.message = None;
        }
        card.last_key = Some(Instant::now());
        card.quiet_since = card.recording.waiting_for_key().then(Instant::now);
        if let Some(hint) = card.recording.hint.clone()
            && Some(&hint) != before_hint.as_ref()
        {
            card.message = Some(HotkeyMessage::Hint(hint));
        }
        match effect {
            Effect::Cancel => self.finish_hotkey(true, false, window, cx),
            Effect::Disable => {
                self.ui.hotkey.mode = HotkeyMode::Saving;
                self.ui.hotkey.candidate.clear();
                self.ui.errors.insert("hotkeyOff".into(), "1".into());
                self.request(
                    "set_hotkey_enabled",
                    json!({"enabled":false}),
                    Some("settings"),
                    window,
                    cx,
                );
            }
            Effect::Commit(hotkey) => self.attempt_hotkey(hotkey, window, cx),
            Effect::None => {}
        }
        cx.notify();
    }

    fn hotkey_modifiers(
        &mut self,
        event: &ModifiersChangedEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.ui.hotkey.mode != HotkeyMode::Recording {
            return;
        }
        cx.stop_propagation();
        let m = event.modifiers;
        let mods = Mods {
            ctrl: m.control,
            alt: m.alt,
            shift: m.shift,
            win: m.platform,
        };
        let card = &mut self.ui.hotkey;
        let capslock = event.capslock.on
            != card.recording.chord.contains(&recorder::ChordKey::Lock)
            && mods == card.recording.mods;
        let before_hint = card.recording.hint.clone();
        if mods != Mods::default()
            && card.recording.mods == Mods::default()
            && card.recording.keys.is_empty()
        {
            card.message = None;
        }
        card.recording.modifiers(mods, capslock);
        card.last_key = Some(Instant::now());
        card.quiet_since = card.recording.waiting_for_key().then(Instant::now);
        if let Some(hint) = card.recording.hint.clone()
            && Some(&hint) != before_hint.as_ref()
        {
            card.message = Some(HotkeyMessage::Hint(hint));
        }
        cx.notify();
    }

    /// The card's timers: 15 s without keys ends a recording; modifiers held
    /// for 1.5 s say the key may be taken; a saved message goes after 1.6 s.
    pub(in crate::frontend) fn tick_hotkey(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let card = &mut self.ui.hotkey;
        let mut changed = false;
        if matches!(card.mode, HotkeyMode::Recording | HotkeyMode::Confirm)
            && card
                .last_key
                .is_some_and(|at| at.elapsed() > Duration::from_millis(HOTKEY_IDLE_MS))
        {
            self.finish_hotkey(true, false, window, cx);
            return true;
        }
        let card = &mut self.ui.hotkey;
        if card.mode == HotkeyMode::Recording
            && card.message.is_none()
            && card
                .quiet_since
                .is_some_and(|at| at.elapsed() > Duration::from_millis(HOTKEY_QUIET_MS))
        {
            card.message = Some(HotkeyMessage::MaybeTaken);
            card.quiet_since = None;
            changed = true;
        }
        if card
            .done
            .as_ref()
            .is_some_and(|(_, _, at)| at.elapsed() > Duration::from_millis(HOTKEY_DONE_MS))
        {
            card.done = None;
            changed = true;
        }
        if card
            .flash
            .is_some_and(|at| at.elapsed() > Duration::from_millis(1200))
        {
            card.flash = None;
        }
        changed
    }

    /// The host gave the hotkey back by itself (window hidden, waited too long).
    pub(in crate::frontend) fn hotkey_changed(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.ui.hotkey.mode != HotkeyMode::Idle {
            self.finish_hotkey(false, false, window, cx);
        }
        self.request("get_settings", json!({}), Some("settings"), window, cx);
    }

    /// The window lost focus: a recording ends, as the original's blur did.
    pub(in crate::frontend) fn hotkey_blur(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.ui.hotkey.mode != HotkeyMode::Idle {
            self.finish_hotkey(true, false, window, cx);
        }
    }

    // ---- Command results --------------------------------------------------------

    pub(in crate::frontend) fn settings_completed(
        &mut self,
        command: &str,
        value: &Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match command {
            "manage_privileged_service" => {
                self.ui.busy.remove("privilege");
                self.data.insert("privilege".into(), value.clone());
                let action = self.ui.errors.remove("privilegeAction").unwrap_or_default();
                if action == "uninstall" {
                    self.notify_toast(ToastKind::Success, label("settings.privilege.uninstalled"));
                } else if service_connected(value) {
                    self.notify_toast(ToastKind::Success, label("settings.privilege.ready"));
                }
                self.request(
                    "task_manager_status",
                    json!({}),
                    Some("taskmgr"),
                    window,
                    cx,
                );
            }
            "manage_task_manager" => {
                self.ui.busy.remove("taskmgr");
                self.data.insert("taskmgr".into(), value.clone());
                let action = self.ui.errors.remove("taskmgrAction").unwrap_or_default();
                self.notify_toast(
                    ToastKind::Success,
                    label(if action == "restore" {
                        "settings.taskManager.restored"
                    } else {
                        "settings.taskManager.enabled"
                    }),
                );
            }
            "set_kernel_sensors" => {
                self.ui.busy.remove("sensors");
            }
            "administrator_status" => {
                self.ui.busy.remove("administratorStatus");
                self.ui.errors.remove("administratorStatus");
            }
            "set_administrator_mode" => {
                self.ui.busy.remove("administrator");
                self.data.insert("administrator".into(), value.clone());
            }
            "clear_history" => {
                self.ui.busy.remove("clearHistory");
                self.notify_toast_for(
                    ToastKind::Success,
                    label("settings.data.history.cleared"),
                    2500,
                );
                self.data.remove("historyUsage");
                self.request("history_usage", json!({}), Some("historyUsage"), window, cx);
            }
            "set_hotkey" => {
                if self.ui.hotkey.mode == HotkeyMode::Saving {
                    let saved = text(value, "hotkey").to_owned();
                    self.succeed_hotkey(false, saved, window, cx);
                }
            }
            "set_hotkey_enabled" => {
                self.ui.hotkey.switching = false;
                if self.ui.errors.remove("hotkeyOff").is_some() {
                    self.succeed_hotkey(true, String::new(), window, cx);
                }
            }
            "install_update" => {
                self.ui.busy.remove("install");
            }
            "set_update_settings" => {
                let proxy = text(value, "updateProxy");
                if crate::updater::PROXY_PRESETS.contains(&proxy) {
                    self.ui.proxy_draft = None;
                    self.ui.inputs.remove("proxyDraft");
                }
            }
            _ => {}
        }
    }

    /// Shows a settings command's failure on its card; false when it is not one.
    pub(in crate::frontend) fn settings_failed(
        &mut self,
        command: &str,
        _args: &Value,
        error: &Value,
        message: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let wrapped = |key: &str| argus_core::i18n::format(key, &[("error", &message)]);
        match command {
            "manage_privileged_service" => {
                self.ui.busy.remove("privilege");
                self.ui.errors.remove("privilegeAction");
                self.ui
                    .errors
                    .insert("privilege".into(), message.to_owned());
                self.data.remove("privilege");
                self.request(
                    "privileged_service_status",
                    json!({}),
                    Some("privilege"),
                    window,
                    cx,
                );
            }
            "manage_task_manager" => {
                self.ui.busy.remove("taskmgr");
                self.ui.errors.remove("taskmgrAction");
                self.ui.errors.insert("taskmgr".into(), message.to_owned());
                self.data.remove("taskmgr");
                self.request(
                    "task_manager_status",
                    json!({}),
                    Some("taskmgr"),
                    window,
                    cx,
                );
            }
            "set_kernel_sensors" => {
                self.ui.busy.remove("sensors");
                self.ui.errors.insert("sensors".into(), message.to_owned());
            }
            "administrator_status" => {
                self.ui.busy.remove("administratorStatus");
                self.ui
                    .errors
                    .insert("administratorStatus".into(), message.to_owned());
            }
            "set_administrator_mode" => {
                // The card reads the state again, showing it as busy meanwhile.
                self.ui.busy.remove("administrator");
                self.ui
                    .errors
                    .insert("administrator".into(), message.to_owned());
                self.ui.busy.insert("administratorStatus".into());
                self.request(
                    "administrator_status",
                    json!({}),
                    Some("administrator"),
                    window,
                    cx,
                );
            }
            "complete_onboarding" | "apply_administrator_mode" => {
                self.ui.busy.remove("onboarding");
                self.notify_toast(ToastKind::Error, wrapped("onboarding.saveFailed"));
            }
            "clear_history" => {
                self.ui.busy.remove("clearHistory");
                self.notify_toast(
                    ToastKind::Error,
                    wrapped("settings.data.history.clearError"),
                );
                self.request("history_usage", json!({}), Some("historyUsage"), window, cx);
            }
            "set_hotkey" | "set_hotkey_enabled" => {
                self.ui.hotkey.switching = false;
                let off = self.ui.errors.remove("hotkeyOff").is_some();
                let kind = text(error, "kind").to_owned();
                self.ui.hotkey.message = Some(HotkeyMessage::Error(error.clone()));
                if command == "set_hotkey"
                    && self.ui.hotkey.mode == HotkeyMode::Saving
                    && matches!(kind.as_str(), "taken" | "failed" | "invalid")
                {
                    // Try another combination, with the modifiers still down kept.
                    let held = &self.ui.hotkey.recording;
                    let from = recorder::Recording {
                        mods: held.mods,
                        chord: held
                            .chord
                            .iter()
                            .filter(|k| matches!(k, recorder::ChordKey::Modifier(_)))
                            .cloned()
                            .collect(),
                        ..Default::default()
                    };
                    self.listen_hotkey(Some(from), window, cx);
                } else if self.ui.hotkey.mode != HotkeyMode::Idle || off {
                    self.finish_hotkey(true, true, window, cx);
                }
            }
            "install_update" => {
                self.ui.busy.remove("install");
                if boolean(error, "cancelled") {
                    self.notify_toast_for(ToastKind::Info, message.to_owned(), 4000);
                } else {
                    self.ui.errors.insert("install".into(), message.to_owned());
                }
            }
            "set_language" => {
                self.notify_toast(ToastKind::Error, wrapped("settings.language.error"))
            }
            "set_theme" => self.notify_toast(ToastKind::Error, wrapped("settings.theme.error")),
            "set_autostart" => self.notify_toast(
                ToastKind::Error,
                wrapped("settings.startup.autostart.error"),
            ),
            "set_close_to_tray" => self.notify_toast(
                ToastKind::Error,
                wrapped("settings.startup.closeToTray.error"),
            ),
            "set_background_intervals" => {
                self.notify_toast(ToastKind::Error, wrapped("settings.data.background.error"))
            }
            "set_history_enabled" => {
                self.notify_toast(ToastKind::Error, wrapped("settings.data.history.error"))
            }
            "set_rules" => self.notify_toast(ToastKind::Error, wrapped("settings.rules.error")),
            "set_update_settings" | "set_whats_new" => {
                self.notify_toast(ToastKind::Error, wrapped("settings.update.error"))
            }
            _ => return false,
        }
        cx.notify();
        true
    }

    // ---- Keyboard shortcuts sheet (ShortcutHelp.tsx) ------------------------

    /// Keys as keycaps: chords split by " / ", keys by "+".
    pub(in crate::frontend) fn shortcut_keys(&self, value: &str) -> AnyElement {
        let p = self.palette;
        let mut row = div().flex().flex_wrap().items_center().gap(px(6.));
        for (index, chord) in value.split(" / ").enumerate() {
            if index > 0 {
                row = row.child(
                    div()
                        .text_size(theme::font(10.))
                        .text_color(p.faint)
                        .child("/"),
                );
            }
            let mut keys = div().flex().items_center().gap(px(4.)).whitespace_nowrap();
            for (part, key) in chord.split('+').enumerate() {
                if part > 0 {
                    keys = keys.child(
                        div()
                            .text_size(theme::font(10.))
                            .text_color(p.faint)
                            .child("+"),
                    );
                }
                keys = keys.child(
                    div()
                        .min_w(px(24.))
                        .h(px(25.))
                        .px(px(6.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .border_1()
                        .border_b_2()
                        .border_color(p.stroke_strong)
                        .rounded(px(5.))
                        .bg(p.face)
                        .text_size(theme::font(11.))
                        .text_color(p.ink)
                        .child(recorder::key_label(key)),
                );
            }
            row = row.child(keys);
        }
        row.into_any_element()
    }

    pub(in crate::frontend) fn shortcuts_dialog(
        &self,
        dialog: gpui_kit::component::dialog::Dialog,
        id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::component::dialog::Dialog {
        let p = self.palette;
        let settings = &self.data["settings"];
        let height = px(760.).min(window.viewport_size().height - px(48.));
        let hotkey = text(settings, "hotkey").to_owned();
        let global = boolean(settings, "hotkeyEnabled")
            && !hotkey.is_empty()
            && settings["hotkeyError"].is_null();
        let open = self
            .modal
            .as_ref()
            .filter(|m| m.id == id)
            .map(|m| m.args["open"].clone())
            .unwrap_or_default();
        let card =
            |glyph: &str, title: String, keys: AnyElement, body: String, badge: Option<String>| {
                div()
                    .flex_1()
                    .min_w_0()
                    .px(px(16.))
                    .py(px(14.))
                    .border_1()
                    .border_color(p.stroke)
                    .rounded(px(8.))
                    .bg(p.layer)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .mb(px(10.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(icon(glyph, p.accent))
                            .child(title)
                            .when_some(badge, |d, badge| {
                                d.child(
                                    div()
                                        .ml_auto()
                                        .text_size(theme::font(10.))
                                        .font_weight(FontWeight::NORMAL)
                                        .text_color(p.faint)
                                        .child(badge),
                                )
                            }),
                    )
                    .child(keys)
                    .child(
                        div()
                            .mt(px(9.))
                            .text_size(theme::font(12.))
                            .line_height(relative(1.6))
                            .text_color(p.muted)
                            .child(body),
                    )
            };
        let common = [
            (
                "PauseIcon",
                "settings.shortcuts.pauseTitle",
                "settings.shortcuts.pause",
                "Ctrl",
            ),
            (
                "EndIcon",
                "common.endProcess",
                "settings.shortcuts.end",
                "Alt+E",
            ),
            (
                "SearchIcon",
                "settings.shortcuts.searchTitle",
                "settings.shortcuts.search",
                "Ctrl+F",
            ),
        ];
        let mut cards = common
            .into_iter()
            .map(|(glyph, title, body, keys)| {
                card(
                    glyph,
                    label(title),
                    self.shortcut_keys(keys),
                    label(body),
                    None,
                )
            })
            .collect::<Vec<_>>();
        cards.push(card(
            "KeyboardIcon",
            label("settings.hotkey.title"),
            if hotkey.is_empty() {
                div()
                    .text_size(theme::font(12.))
                    .text_color(p.faint)
                    .child(label("settings.shortcuts.disabled"))
                    .into_any_element()
            } else {
                self.shortcut_keys(&recorder::shortcut_keys(&hotkey).join("+"))
            },
            label(if global {
                "settings.shortcuts.globalHint"
            } else {
                "settings.shortcuts.disabled"
            }),
            Some(label("settings.shortcuts.global")),
        ));
        let mut rows = div().flex().flex_col().gap(px(10.));
        let mut iter = cards.into_iter();
        while let Some(first) = iter.next() {
            let mut row = div().flex().gap(px(10.)).child(first);
            if let Some(second) = iter.next() {
                row = row.child(second);
            }
            rows = rows.child(row);
        }
        let pages = div()
            .flex()
            .flex_wrap()
            .gap_y(px(8.))
            .children(PAGES.iter().enumerate().map(|(index, (page, glyph))| {
                div()
                    .w(relative(0.25))
                    .pr(px(14.))
                    .flex()
                    .items_center()
                    .gap(px(7.))
                    .min_w_0()
                    .child(icon(glyph, p.muted).size(px(15.)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(label(&format!("shell.page.{page}"))),
                    )
                    .child(self.shortcut_keys(&format!("Ctrl+{}", index + 1)))
            }));
        type Group<'a> = (&'a str, &'a str, &'a str, &'a [(&'a str, &'a str)]);
        let groups: [Group; 2] = [
            (
                "table",
                "ProcessesIcon",
                "settings.shortcuts.table",
                &[
                    ("↑ / ↓", "settings.shortcuts.select"),
                    ("PageUp / PageDown", "settings.shortcuts.page"),
                    ("Home / End", "settings.shortcuts.ends"),
                    ("← / →", "settings.shortcuts.expand"),
                    ("Enter", "settings.shortcuts.details"),
                    ("Delete", "settings.shortcuts.endTable"),
                    ("Shift+Delete", "settings.shortcuts.endDirect"),
                    ("Esc", "settings.shortcuts.clear"),
                ],
            ),
            (
                "history",
                "HistoryIcon",
                "settings.shortcuts.history",
                &[
                    ("← / →", "settings.shortcuts.time"),
                    ("Shift+← / Shift+→", "settings.shortcuts.timeJump"),
                    ("PageUp / PageDown", "settings.shortcuts.timeJump"),
                    ("Home / End", "settings.shortcuts.timeEnds"),
                    ("Esc", "settings.shortcuts.latest"),
                ],
            ),
        ];
        let mut body = div()
            .flex()
            .flex_col()
            .pt(px(20.))
            .px(px(24.))
            .pb(px(20.))
            .child(rows)
            .child(
                div()
                    .flex()
                    .justify_between()
                    .items_center()
                    .mt(px(20.))
                    .mb(px(10.))
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(label("settings.shortcuts.navigation")),
                    )
                    .child(self.shortcut_keys("Ctrl+1–7")),
            )
            .child(div().mb(px(20.)).child(pages));
        for (key, glyph, title, items) in groups {
            let expanded = boolean(&open, key);
            body = body.child(
                div()
                    .mb(px(8.))
                    .border_1()
                    .border_color(p.stroke)
                    .rounded(px(8.))
                    .bg(p.layer)
                    .child(
                        gpui_kit::base::Button::new(SharedString::from(format!(
                            "shortcut-group-{key}"
                        )))
                        .w_full()
                        .h(px(40.))
                        .px(px(14.))
                        .gap(px(8.))
                        .justify_start()
                        .rounded(px(8.))
                        .hover(move |s| s.bg(p.hover))
                        .child(icon(glyph, p.accent))
                        .child(
                            div()
                                .flex_1()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(label(title)),
                        )
                        .child(
                            icon(if expanded { "ChevronUp" } else { "ChevronDown" }, p.muted)
                                .size(px(14.)),
                        )
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                if let Some(modal) = &mut this.modal
                                    && modal.id == id
                                {
                                    let now = !boolean(&modal.args["open"], key);
                                    if !modal.args["open"].is_object() {
                                        modal.args["open"] = json!({});
                                    }
                                    modal.args["open"][key] = json!(now);
                                }
                                window.refresh();
                                cx.notify();
                            },
                        )),
                    )
                    .when(expanded, |d| {
                        d.child(
                            div()
                                .px(px(14.))
                                .pb(px(12.))
                                .flex()
                                .flex_col()
                                .gap(px(8.))
                                .children(items.iter().map(|(keys, text_key)| {
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .gap(px(16.))
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w_0()
                                                .text_size(theme::font(12.))
                                                .text_color(p.muted)
                                                .child(label(text_key)),
                                        )
                                        .child(self.shortcut_keys(keys))
                                })),
                        )
                    }),
            );
        }
        body = body
            .child(
                div()
                    .mt(px(12.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .text_size(theme::font(12.))
                    .text_color(p.muted)
                    .child(self.shortcut_keys("Ctrl+Shift+Esc"))
                    .child(label("settings.shortcuts.taskManager")),
            )
            .child(
                div()
                    .mt(px(12.))
                    .text_size(theme::font(12.))
                    .text_color(p.faint)
                    .child(label("settings.shortcuts.inputHint")),
            );
        let header = div()
            .flex()
            .items_center()
            .gap(px(14.))
            .pt(px(22.))
            .px(px(24.))
            .pb(px(18.))
            .border_b_1()
            .border_color(p.stroke)
            .child(
                div()
                    .size(px(44.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .border_1()
                    .border_color(p.accent.opacity(0.24))
                    .rounded(px(12.))
                    .bg(p.accent.opacity(0.1))
                    .child(icon("KeyboardIcon", p.accent).size(px(24.))),
            )
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(self.dialog_title(label("settings.shortcuts.title")))
                    .child(
                        div()
                            .text_size(theme::font(12.))
                            .text_color(p.muted)
                            .child(label("settings.shortcuts.scope")),
                    ),
            )
            .child(
                self.btn(
                    format!("modal-{id}-x"),
                    "",
                    Some("DismissIcon"),
                    Tone::Plain,
                )
                .accessibility_label(label("common.close"))
                .on_click(cx.listener(|this, _, window, cx| this.close_overlay(window, cx))),
            );
        dialog
            .h(height)
            .child(
                self.measure(id, 0)
                    .child(div().flex().flex_col().child(header).child(body)),
            )
            .footer(
                self.measure(id, 1).child(
                    self.dialog_footer()
                        .justify_between()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .text_size(theme::font(12.))
                                .text_color(p.muted)
                                .child(self.shortcut_keys("Esc"))
                                .child(label("settings.shortcuts.close")),
                        )
                        .child(
                            self.btn(
                                format!("modal-{id}-close"),
                                label("common.close"),
                                None,
                                Tone::Accent,
                            )
                            .min_w(px(96.))
                            .track_focus(&self.focus_for(&format!("modal-{id}-primary"), cx))
                            .on_click(
                                cx.listener(|this, _, window, cx| this.close_overlay(window, cx)),
                            ),
                        ),
                ),
            )
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
