//! Data and history: the sampling interval and the background intervals
//! (the tray's is on the Components page), freeing memory while the window
//! is hidden, the byte and network units and clearing the history. The
//! history's own cards are in `history::settings`.
use super::*;
use crate::frontend::units::{self, ByteUnit, MEGA};

/// The setting that holds the background interval for `away`
/// ("minimized" or "tray").
fn background_field(away: &str) -> &'static str {
    if away == "minimized" {
        "minimizedInterval"
    } else {
        "trayInterval"
    }
}

impl Desktop {
    // ---- Data -------------------------------------------------------------------

    pub(super) fn data_cards(
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
                .map(|ms| {
                    tr!(
                        "shell.interval.option",
                        seconds = crate::regional::seconds(*ms)
                    )
                })
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
        // The tray's interval is on the Components page.
        cards.push(self.background_card(
            "settings",
            "minimizedInterval",
            "MinimizeIcon",
            "minimized",
            cx,
        ));
        let minutes = self.release_hidden_minutes();
        let release = self.select(
            "settings-release-hidden",
            label("settings.data.releaseHidden.title"),
            RELEASE_HIDDEN_MINUTES
                .iter()
                .map(|m| match m {
                    0 => label("settings.data.releaseHidden.never"),
                    _ => tr!("settings.data.releaseHidden.option", minutes = *m),
                })
                .collect(),
            RELEASE_HIDDEN_MINUTES.iter().position(|m| *m == minutes),
            false,
            move |this, index, window, cx| {
                this.preference(
                    "memory.releaseHidden",
                    json!(RELEASE_HIDDEN_MINUTES[index]),
                    window,
                    cx,
                );
            },
            cx,
        );
        cards.push(self.setting_text(
            "releaseHidden",
            "MemoryIcon",
            "settings.data.releaseHidden.title",
            "settings.data.releaseHidden.desc",
            vec![release],
        ));
        let byte_unit = ByteUnit::load(&self.prefs);
        let byte_units = self.select(
            "settings-byte-unit",
            label("settings.data.byteUnit.title"),
            ByteUnit::ALL
                .iter()
                .map(|unit| label(&format!("settings.data.byteUnit.{}", unit.key())))
                .collect(),
            ByteUnit::ALL.iter().position(|unit| *unit == byte_unit),
            false,
            move |this, index, window, cx| {
                this.preference(
                    units::PREFERENCE,
                    json!(ByteUnit::ALL[index].key()),
                    window,
                    cx,
                );
            },
            cx,
        );
        cards.push(self.setting_text(
            "byteUnit",
            "ByteUnitIcon",
            "settings.data.byteUnit.title",
            "settings.data.byteUnit.desc",
            vec![byte_units],
        ));
        let unit = self.prefs.get::<String>("netUnit", "bytes".into());
        let units = self.segments(
            "setting-netUnit",
            vec![
                (
                    byte_unit.rate_symbol(MEGA).into(),
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
        self.history_setting_cards(settings, cards, window, cx);
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

    /// How often to sample while minimized or in the tray; under 5 s only
    /// after a warning. `superseded` while a monitor keeps sampling at the
    /// refresh interval (`background_superseded`): faded, yet it still takes
    /// a choice for when the interval applies again.
    pub(in crate::frontend) fn background_select(
        &self,
        place: &str,
        away: &'static str,
        superseded: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let settings = &self.data["settings"];
        let current = settings[background_field(away)].as_u64();
        let choices = crate::settings::BACKGROUND_INTERVALS;
        self.select_superseded(
            format!("{place}-background-{away}"),
            label(&format!("settings.data.{away}.title")),
            choices
                .iter()
                .map(|ms| tr!("shell.interval.option", seconds = crate::regional::seconds(*ms)))
                .collect(),
            choices.iter().position(|ms| Some(*ms) == current),
            superseded,
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
                    let seconds = crate::regional::seconds(ms);
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

    /// A background interval's card. While the interval does not apply (the
    /// desktop or taskbar monitor keeps sampling at the refresh interval,
    /// `shell::sampling_state`), its drop-down is faded and a note under the
    /// description says which monitor and how often it samples instead.
    pub(in crate::frontend) fn background_card(
        &self,
        place: &str,
        id: &str,
        glyph: &str,
        away: &'static str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let superseded = self.background_superseded(away);
        let select = self.background_select(place, away, superseded.is_some(), cx);
        let desc = div()
            .child(label(&format!("settings.data.{away}.desc")))
            .children(superseded.map(|monitors| self.background_pace_note(monitors)));
        self.setting(
            id,
            self.glyph(glyph),
            label(&format!("settings.data.{away}.title")),
            Some(desc.into_any_element()),
            None,
            vec![select],
        )
    }

    /// Whether the desktop and the taskbar monitor are on, when either is
    /// and so keeps sampling at the refresh interval instead of `away`'s;
    /// none while that is the refresh interval anyway.
    fn background_superseded(&self, away: &str) -> Option<(bool, bool)> {
        let settings = &self.data["settings"];
        if settings[background_field(away)].as_u64() == Some(self.interval) {
            return None;
        }
        // As `sync_monitors` and `shell::apply_taskbar_monitor` decide.
        let desktop = &settings["desktopMonitor"];
        let desktop = desktop["enabled"].as_bool() == Some(true)
            && desktop["metrics"].as_array().is_some_and(|m| !m.is_empty());
        let taskbar = settings["taskbarMonitor"]["enabled"].as_bool() == Some(true);
        (desktop || taskbar).then_some((desktop, taskbar))
    }

    /// Which monitors keep the refresh interval, and how often that samples.
    fn background_pace_note(&self, (desktop, taskbar): (bool, bool)) -> AnyElement {
        let seconds = crate::regional::seconds(self.interval);
        let text = match (desktop, taskbar) {
            (true, true) => tr!("settings.data.monitorPace.both", seconds = seconds),
            (true, false) => tr!("settings.data.monitorPace.desktop", seconds = seconds),
            _ => tr!("settings.data.monitorPace.taskbar", seconds = seconds),
        };
        let p = self.palette;
        div()
            .mt(px(6.))
            .flex()
            .gap(px(6.))
            .text_color(p.ink)
            .child(
                div()
                    .flex_shrink_0()
                    .mt(px(1.))
                    .child(icon("InfoIcon", p.accent)),
            )
            .child(div().flex_1().min_w_0().child(text))
            .into_any_element()
    }
}
