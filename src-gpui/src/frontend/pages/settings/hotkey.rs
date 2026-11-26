//! The show/hide hotkey card: recording a combination, confirming or refusing
//! it, saving it or switching it off, and the timers of the recording.
use super::*;
use crate::frontend::recorder::{self, Effect, Mods, Verdict};
use crate::frontend::recorder::{hotkey_error_text, issue_text};
use crate::frontend::widgets::{ButtonExt, HotkeyMessage, HotkeyMode, Tone};

/// Settings in the update section, by the setting a notice points to.
const HOTKEY_IDLE_MS: u64 = 15_000;
const HOTKEY_QUIET_MS: u64 = 1_500;
const HOTKEY_DONE_MS: u64 = 1_600;

impl Desktop {
    // ---- Hotkey card ------------------------------------------------------------

    /// The show/hide hotkey: the combination and a switch on top; below,
    /// while recording, confirming or refusing, what is going on.
    pub(super) fn hotkey_card(
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
    pub(super) fn listen_hotkey(
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

    pub(super) fn succeed_hotkey(
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
        let mut mods = Mods {
            ctrl: m.control,
            alt: m.alt,
            shift: m.shift,
            win: m.platform,
        };
        let mut key = event.keystroke.key.clone();
        if let Some((code, [ctrl, alt, shift, win])) = crate::hotkey::recording_key(down) {
            key = code.to_owned();
            mods = Mods {
                ctrl,
                alt,
                shift,
                win,
            };
        }
        let card = &mut self.ui.hotkey;
        let before_held = card.recording.mods != Mods::default() || !card.recording.keys.is_empty();
        let before_hint = card.recording.hint.clone();
        let effect = if down {
            card.recording.key_down(&key, mods, event.is_held)
        } else {
            card.recording.key_up(&key)
        };
        // Tab still moves the focus as it ends the recording.
        if !(effect == Effect::Cancel && key.eq_ignore_ascii_case("tab")) {
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

    /// The window lost focus: a recording ends, as the interface's blur did.
    pub(in crate::frontend) fn hotkey_blur(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.ui.hotkey.mode != HotkeyMode::Idle {
            self.finish_hotkey(true, false, window, cx);
        }
    }
}
