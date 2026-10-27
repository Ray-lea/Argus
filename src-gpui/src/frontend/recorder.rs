//! The show/hide hotkey's rules and recorder, after the original lib/hotkey.ts.
//!
//! The table is shared with the host (hotkey.rs refuses the same `block`
//! combinations); this side also warns about combinations the host accepts.
//! GPUI reports modifiers without their side and printable keys as layout
//! characters, so the recorder maps each key back to a table code and keeps
//! the original's order: modifiers first, committed when the main key comes up
//! (committing on the way down would register the hotkey while the key is
//! still held, and its auto-repeat would fire it).
use serde::Deserialize;
use std::sync::LazyLock;

#[derive(Deserialize)]
struct Table {
    keys: Vec<Key>,
    reserved: Vec<String>,
    #[serde(rename = "taskManager")]
    task_manager: Vec<String>,
    editing: Vec<String>,
}

#[derive(Deserialize)]
struct Key {
    code: String,
    group: String,
    #[serde(default)]
    label: Option<String>,
}

static TABLE: LazyLock<Table> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../../resources/hotkey-table.json"))
        .expect("hotkey-table.json")
});

fn key_by_code(code: &str) -> Option<&'static Key> {
    TABLE
        .keys
        .iter()
        .find(|k| k.code.eq_ignore_ascii_case(code))
}

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Mods {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
}
impl Mods {
    fn any(&self) -> bool {
        self.ctrl || self.alt || self.shift || self.win
    }
}

struct Accelerator {
    mods: Mods,
    key: &'static Key,
}

fn modifier(token: &str) -> Option<fn(&mut Mods)> {
    Some(match token.to_ascii_lowercase().as_str() {
        "ctrl" | "control" | "commandorcontrol" | "commandorctrl" | "cmdorctrl"
        | "cmdorcontrol" => |m| m.ctrl = true,
        "alt" | "option" => |m| m.alt = true,
        "shift" => |m| m.shift = true,
        "super" | "win" | "meta" | "cmd" | "command" => |m| m.win = true,
        _ => return None,
    })
}

fn key_from_token(token: &str) -> Option<&'static Key> {
    let lower = token.to_ascii_lowercase();
    let name = match lower.as_str() {
        "esc" => "Escape".to_string(),
        "numpadenter" => "Enter".to_string(),
        "up" | "down" | "left" | "right" => format!("Arrow{lower}"),
        s if s.len() == 1 && s.as_bytes()[0].is_ascii_lowercase() => format!("Key{s}"),
        s if s.len() == 1 && s.as_bytes()[0].is_ascii_digit() => format!("Digit{s}"),
        _ => lower,
    };
    key_by_code(&name)
}

fn parse(text: &str) -> Result<Accelerator, &'static str> {
    let tokens: Vec<&str> = text.split('+').map(str::trim).collect();
    if tokens.iter().any(|t| t.is_empty()) {
        return Err("invalid");
    }
    let (last, rest) = tokens.split_last().ok_or("invalid")?;
    let mut mods = Mods::default();
    for token in rest {
        modifier(token).ok_or("invalid")?(&mut mods);
    }
    if modifier(last).is_some() {
        return Err("noKey");
    }
    let key = key_from_token(last).ok_or("unsupported")?;
    Ok(Accelerator { mods, key })
}

fn canonical(mods: Mods, code: &str) -> String {
    let mut parts = Vec::new();
    for (on, name) in [
        (mods.ctrl, "Ctrl"),
        (mods.alt, "Alt"),
        (mods.shift, "Shift"),
        (mods.win, "Super"),
    ] {
        if on {
            parts.push(name);
        }
    }
    parts.push(code);
    parts.join("+")
}

/// What the rules say about a combination.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    Ok,
    /// Refused, for the reason named (a `settings.hotkey.issue.*` key).
    Block(&'static str),
    /// Allowed after a confirmation (`settings.hotkey.warning.*` keys).
    Warn(Vec<&'static str>),
}

/// Parses and judges a combination in one go.
pub fn judge(text: &str) -> Verdict {
    let a = match parse(text) {
        Ok(a) => a,
        Err(issue) => return Verdict::Block(issue),
    };
    let key = a.key;
    match key.group.as_str() {
        "media" => return Verdict::Block("unsupported"),
        "lock" => return Verdict::Block("lockKey"),
        _ => {}
    }
    let name = canonical(a.mods, &key.code);
    if TABLE.reserved.contains(&name) {
        return Verdict::Block("reserved");
    }
    if TABLE.task_manager.contains(&name) {
        return Verdict::Block("taskManager");
    }
    if TABLE.editing.contains(&name) {
        return Verdict::Block("editing");
    }
    // With Ctrl down the keyboard sends Break, which Windows reports as
    // VK_CANCEL: a hotkey on Ctrl+…+Pause would never fire.
    if key.code == "Pause" && a.mods.ctrl {
        return Verdict::Block("unsupported");
    }
    let m = a.mods;
    let shift_function = m.shift && !m.ctrl && !m.alt && !m.win && key.group == "function";
    if !m.ctrl && !m.alt && !m.win && key.group != "extra" && !shift_function {
        return Verdict::Block("needsModifier");
    }
    let in_app = (1..=7)
        .any(|n| name == format!("Ctrl+Digit{n}") || name == format!("Ctrl+Numpad{n}"))
        || name == "Ctrl+KeyF"
        || name == "Ctrl+Shift+KeyF";
    let f_key = key.group == "function" || key.group == "extra";
    let mut warnings = Vec::new();
    if in_app {
        warnings.push("inApp");
    } else if name == "Ctrl+Space" {
        warnings.push("imeSwitch");
    } else if shift_function || (!m.win && !(m.ctrl && m.alt) && !f_key) {
        warnings.push("common");
    }
    if key.group == "numpad" {
        warnings.push("numpad");
    }
    if key.code == "PrintScreen" {
        warnings.push("printScreen");
    }
    if warnings.is_empty() {
        Verdict::Ok
    } else {
        Verdict::Warn(warnings)
    }
}

/// How a key is printed: `KeyK` → `K`, `Digit7` → `7`, `Escape` → `Esc`.
fn key_name(key: &Key) -> String {
    if let Some(label) = &key.label {
        return label.clone();
    }
    match key.group.as_str() {
        "letter" => key.code[3..].to_owned(),
        "digit" => key.code[5..].to_owned(),
        _ => key.code.clone(),
    }
}

/// One part of an accelerator as printed on a keyboard.
pub fn key_label(part: &str) -> String {
    match part.to_ascii_lowercase().as_str() {
        "ctrl" | "control" => return "Ctrl".into(),
        "alt" => return "Alt".into(),
        "shift" => return "Shift".into(),
        "super" | "win" | "meta" => return "Win".into(),
        _ => {}
    }
    let code = if part.eq_ignore_ascii_case("numpadenter") {
        "Enter"
    } else {
        part
    };
    match key_by_code(code) {
        Some(key) if key.code == "Space" => super::theme::label("common.keys.space"),
        Some(key) => key_name(key),
        None => part.to_owned(),
    }
}

/// The keys of an accelerator, in order; empty when there is none.
pub fn shortcut_keys(accelerator: &str) -> Vec<String> {
    if accelerator.is_empty() {
        return vec![];
    }
    accelerator
        .split('+')
        .map(|part| key_label(part.trim()))
        .collect()
}

/// An accelerator as text, e.g. `Ctrl+Alt+Esc`.
pub fn shortcut_text(accelerator: &str) -> String {
    shortcut_keys(accelerator).join("+")
}

/// Why nothing was committed, or what the keys did not do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Hint {
    NoKey,
    Order,
    LockKey,
    MultipleKeys,
    Unsupported(String),
}

/// A key of the chord being recorded, as shown while recording.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChordKey {
    Modifier(&'static str),
    Lock,
    Key(String),
}

#[derive(Clone, Default, Debug)]
pub struct Recording {
    /// Modifiers down now.
    pub mods: Mods,
    /// Ordinary keys down now, by GPUI key name.
    pub keys: Vec<String>,
    /// Every key pressed since the keys were last all up, in order.
    pub chord: Vec<ChordKey>,
    /// The main key and the combination it makes, until it comes up.
    pub pending: Option<(String, String, Mods)>,
    /// A modifier went down after the main key.
    pub order: bool,
    pub hint: Option<Hint>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    None,
    Cancel,
    Disable,
    Commit(String),
}

/// The table code for a GPUI key name; `shift` says whether the layout's
/// shifted character stood for the key with Shift held.
fn table_code(key: &str) -> Option<(String, bool)> {
    let plain = |code: &str| Some((code.to_owned(), false));
    let shifted = |code: &str| Some((code.to_owned(), true));
    match key {
        k if k.len() == 1 && k.as_bytes()[0].is_ascii_lowercase() => {
            Some((format!("Key{}", k.to_ascii_uppercase()), false))
        }
        k if k.len() == 1 && k.as_bytes()[0].is_ascii_uppercase() => {
            Some((format!("Key{k}"), true))
        }
        k if k.len() == 1 && k.as_bytes()[0].is_ascii_digit() => Some((format!("Digit{k}"), false)),
        k if k.len() >= 2
            && k.starts_with('f')
            && k[1..].parse::<u8>().is_ok_and(|n| (1..=24).contains(&n)) =>
        {
            Some((format!("F{}", &k[1..]), false))
        }
        "escape" => plain("Escape"),
        "enter" => plain("Enter"),
        "space" => plain("Space"),
        "backspace" => plain("Backspace"),
        "tab" => plain("Tab"),
        "menu" => plain("ContextMenu"),
        "up" => plain("ArrowUp"),
        "down" => plain("ArrowDown"),
        "left" => plain("ArrowLeft"),
        "right" => plain("ArrowRight"),
        "home" => plain("Home"),
        "end" => plain("End"),
        "pageup" => plain("PageUp"),
        "pagedown" => plain("PageDown"),
        "insert" => plain("Insert"),
        "delete" => plain("Delete"),
        "`" => plain("Backquote"),
        "-" => plain("Minus"),
        "=" => plain("Equal"),
        "[" => plain("BracketLeft"),
        "]" => plain("BracketRight"),
        "\\" => plain("Backslash"),
        ";" => plain("Semicolon"),
        "'" => plain("Quote"),
        "," => plain("Comma"),
        "." => plain("Period"),
        "/" => plain("Slash"),
        "~" => shifted("Backquote"),
        "!" => shifted("Digit1"),
        "@" => shifted("Digit2"),
        "#" => shifted("Digit3"),
        "$" => shifted("Digit4"),
        "%" => shifted("Digit5"),
        "^" => shifted("Digit6"),
        "&" => shifted("Digit7"),
        "*" => shifted("Digit8"),
        "(" => shifted("Digit9"),
        ")" => shifted("Digit0"),
        "_" => shifted("Minus"),
        "+" => shifted("Equal"),
        "{" => shifted("BracketLeft"),
        "}" => shifted("BracketRight"),
        "|" => shifted("Backslash"),
        ":" => shifted("Semicolon"),
        "\"" => shifted("Quote"),
        "<" => shifted("Comma"),
        ">" => shifted("Period"),
        "?" => shifted("Slash"),
        _ => None,
    }
}

impl Recording {
    /// The modifiers changed (GPUI reports them apart from other keys).
    pub fn modifiers(&mut self, mods: Mods, capslock_changed: bool) -> Effect {
        if capslock_changed {
            if self.mods == Mods::default() && self.keys.is_empty() {
                self.chord.clear();
                self.order = false;
            }
            if !self.chord.contains(&ChordKey::Lock) {
                self.chord.push(ChordKey::Lock);
            }
            self.hint = Some(Hint::LockKey);
            return Effect::None;
        }
        let before = self.mods;
        if !before.any() && self.keys.is_empty() && mods.any() {
            self.chord.clear();
            self.order = false;
            self.hint = None;
        }
        for (was, now, name) in [
            (before.ctrl, mods.ctrl, "Ctrl"),
            (before.alt, mods.alt, "Alt"),
            (before.shift, mods.shift, "Shift"),
            (before.win, mods.win, "Win"),
        ] {
            if now && !was {
                if !self.chord.contains(&ChordKey::Modifier(name)) {
                    self.chord.push(ChordKey::Modifier(name));
                }
                // A modifier the main key went down without: pressed too late.
                if let Some((_, _, held)) = &self.pending {
                    let had = match name {
                        "Ctrl" => held.ctrl,
                        "Alt" => held.alt,
                        "Shift" => held.shift,
                        _ => held.win,
                    };
                    if !had {
                        self.order = true;
                    }
                }
            }
        }
        self.mods = mods;
        self.settle()
    }

    /// A key other than a modifier went down.
    pub fn key_down(&mut self, key: &str, mods: Mods, repeat: bool) -> Effect {
        if repeat {
            return Effect::None;
        }
        if !self.mods.any() && self.keys.is_empty() {
            self.chord.clear();
            self.order = false;
            self.hint = None;
        }
        self.mods = Mods {
            shift: self.mods.shift || mods.shift,
            ..mods
        };
        // Tab and Shift+Tab move the focus on, out of the recording.
        if key == "tab" && !mods.ctrl && !mods.alt && !mods.win {
            *self = Recording::default();
            return Effect::Cancel;
        }
        if !mods.any() && self.keys.is_empty() {
            if key == "escape" {
                *self = Recording::default();
                return Effect::Cancel;
            }
            if key == "backspace" {
                *self = Recording::default();
                return Effect::Disable;
            }
        }
        let others = self.keys.iter().any(|k| k != key);
        if !self.keys.iter().any(|k| k == key) {
            self.keys.push(key.to_owned());
        }
        let mapped = table_code(key);
        let label = mapped
            .as_ref()
            .and_then(|(code, _)| key_by_code(code))
            .map(key_name)
            .unwrap_or_else(|| key.to_uppercase());
        let entry = ChordKey::Key(label.clone());
        if !self.chord.contains(&entry) {
            self.chord.push(entry);
        }
        // Another ordinary key still down: a hotkey is modifiers and one key.
        if others {
            self.pending = None;
            self.hint = Some(Hint::MultipleKeys);
            return Effect::None;
        }
        let Some((code, shifted)) = mapped else {
            self.pending = None;
            self.hint = Some(Hint::Unsupported(label));
            return Effect::None;
        };
        let held = Mods {
            shift: mods.shift || shifted,
            ..mods
        };
        self.pending = Some((key.to_owned(), canonical(held, &code), held));
        self.order = false;
        self.hint = None;
        Effect::None
    }

    /// A key other than a modifier came up.
    pub fn key_up(&mut self, key: &str) -> Effect {
        self.keys.retain(|k| k != key);
        if let Some((pending, hotkey, _)) = self.pending.clone()
            && pending == key
        {
            self.pending = None;
            if self.order {
                self.hint = Some(Hint::Order);
                return Effect::None;
            }
            return Effect::Commit(hotkey);
        }
        self.settle()
    }

    /// Everything up and no main key: modifiers alone (CapsLock aside).
    fn settle(&mut self) -> Effect {
        let only_modifiers = self
            .chord
            .iter()
            .any(|k| matches!(k, ChordKey::Modifier(_)))
            && !self.chord.iter().any(|k| matches!(k, ChordKey::Key(_)));
        if !self.mods.any()
            && self.keys.is_empty()
            && only_modifiers
            && matches!(self.hint, None | Some(Hint::LockKey))
        {
            self.hint = Some(Hint::NoKey);
        }
        Effect::None
    }

    /// Modifiers are held without an ordinary key (Shift alone aside): the
    /// main key may be one that Windows or another program takes first.
    pub fn waiting_for_key(&self) -> bool {
        self.pending.is_none()
            && self.keys.is_empty()
            && (self.mods.ctrl || self.mods.alt || self.mods.win)
    }
}

/// Why `shortcut` is refused (keys.ts, hotkeyIssueText).
pub(in crate::frontend) fn issue_text(issue: &str, shortcut: &str) -> String {
    let key = shortcut.rsplit('+').next().unwrap_or_default().trim();
    argus_core::i18n::format(
        &format!("settings.hotkey.issue.{issue}"),
        &[
            ("shortcut", &shortcut_text(shortcut)),
            ("key", &key_label(key)),
        ],
    )
}

/// Why the hotkey is not active or was not changed (keys.ts, hotkeyErrorText).
pub(in crate::frontend) fn hotkey_error_text(error: &serde_json::Value) -> Option<String> {
    if error.is_null() {
        return None;
    }
    let Some(kind) = error["kind"].as_str() else {
        return error.as_str().map(str::to_owned);
    };
    let shortcut = shortcut_text(value_text(error, "shortcut"));
    Some(match kind {
        "invalid" => argus_core::tr!(
            "settings.hotkey.error.invalid",
            shortcut = shortcut,
            reason = issue_text(value_text(error, "issue"), value_text(error, "shortcut"))
        ),
        "taken" => argus_core::tr!("settings.hotkey.error.taken", shortcut = shortcut),
        "failed" => argus_core::tr!(
            "settings.hotkey.error.failed",
            shortcut = shortcut,
            code = error["code"]
        ),
        _ => super::theme::label("settings.hotkey.error.saveFailed"),
    })
}

fn value_text<'a>(value: &'a serde_json::Value, key: &str) -> &'a str {
    value[key].as_str().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The cases the host tests too, including the warnings it does not give.
    #[test]
    fn verdicts_match_the_shared_cases() {
        #[derive(Deserialize)]
        struct Case {
            hotkey: String,
            level: String,
            #[serde(default)]
            issue: Option<String>,
            #[serde(default)]
            warnings: Vec<String>,
        }
        let cases: Vec<Case> =
            serde_json::from_str(include_str!("../../tests/hotkey-cases.json")).unwrap();
        for case in cases {
            let verdict = judge(&case.hotkey);
            match case.level.as_str() {
                "block" => assert_eq!(
                    verdict,
                    Verdict::Block(match case.issue.as_deref().unwrap() {
                        "invalid" => "invalid",
                        "noKey" => "noKey",
                        "unsupported" => "unsupported",
                        "lockKey" => "lockKey",
                        "needsModifier" => "needsModifier",
                        "reserved" => "reserved",
                        "taskManager" => "taskManager",
                        other => {
                            assert_eq!(other, "editing", "{}", case.hotkey);
                            "editing"
                        }
                    }),
                    "{}",
                    case.hotkey
                ),
                "warn" => {
                    let Verdict::Warn(warnings) = verdict else {
                        panic!("{} should warn", case.hotkey);
                    };
                    assert_eq!(warnings, case.warnings, "{}", case.hotkey);
                }
                _ => assert_eq!(verdict, Verdict::Ok, "{}", case.hotkey),
            }
        }
    }

    #[test]
    fn records_modifiers_then_commits_on_key_up() {
        let mut r = Recording::default();
        let ctrl_alt = Mods {
            ctrl: true,
            alt: true,
            ..Mods::default()
        };
        assert_eq!(r.modifiers(ctrl_alt, false), Effect::None);
        assert!(r.waiting_for_key());
        assert_eq!(r.key_down("k", ctrl_alt, false), Effect::None);
        assert_eq!(r.key_down("k", ctrl_alt, true), Effect::None);
        assert_eq!(r.key_up("k"), Effect::Commit("Ctrl+Alt+KeyK".into()));
    }

    #[test]
    fn a_late_modifier_is_an_order_hint() {
        let mut r = Recording::default();
        let ctrl = Mods {
            ctrl: true,
            ..Mods::default()
        };
        r.key_down("k", ctrl, false);
        r.modifiers(Mods { alt: true, ..ctrl }, false);
        assert_eq!(r.key_up("k"), Effect::None);
        assert_eq!(r.hint, Some(Hint::Order));
    }

    #[test]
    fn escape_cancels_and_backspace_turns_off() {
        let mut r = Recording::default();
        assert_eq!(r.key_down("escape", Mods::default(), false), Effect::Cancel);
        assert_eq!(
            r.key_down("backspace", Mods::default(), false),
            Effect::Disable
        );
    }

    #[test]
    fn shifted_symbols_record_their_key_with_shift() {
        let mut r = Recording::default();
        let ctrl = Mods {
            ctrl: true,
            ..Mods::default()
        };
        r.key_down("!", ctrl, false);
        assert_eq!(r.key_up("!"), Effect::Commit("Ctrl+Shift+Digit1".into()));
    }

    #[test]
    fn modifiers_alone_say_a_key_is_missing() {
        let mut r = Recording::default();
        let ctrl = Mods {
            ctrl: true,
            ..Mods::default()
        };
        r.modifiers(ctrl, false);
        r.modifiers(Mods::default(), false);
        assert_eq!(r.hint, Some(Hint::NoKey));
    }
}
