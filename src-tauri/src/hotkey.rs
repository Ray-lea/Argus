//! The show/hide hotkey: which combinations are accepted, and their
//! registration with Windows.
//!
//! RegisterHotKey compares the exact set of Ctrl, Alt, Shift and Win (either
//! side of each) when the main key goes down. It cannot tell left from right,
//! take modifiers alone or use CapsLock as a modifier (measured: a hotkey on
//! VK_RCONTROL never fires, Ctrl+VK_CONTROL fires on either Ctrl alone). The
//! keys and the combinations refused are in `ui/src/lib/hotkey-table.json`,
//! which the settings page reads too; the page also warns about combinations
//! this side accepts.
//!
//! Registration goes to the main window and runs on the main thread, which
//! owns it; synchronous commands run there.

use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::{LazyLock, Mutex, OnceLock};
use std::thread::ThreadId;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, Runtime, WebviewWindow};
use windows::Win32::Foundation::{ERROR_HOTKEY_ALREADY_REGISTERED, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    HOT_KEY_MODIFIERS, MOD_NOREPEAT, RegisterHotKey, UnregisterHotKey,
};
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::WM_HOTKEY;

use crate::settings::SettingsStore;

// RegisterHotKey's modifier bits.
const ALT: u32 = 1;
const CTRL: u32 = 2;
const SHIFT: u32 = 4;
const WIN: u32 = 8;

/// Tells the page that the hotkey changed without it asking (a suspended one
/// came back, a retry succeeded).
const CHANGED_EVENT: &str = "argus-hotkey-changed";
/// A suspended hotkey comes back on its own after this, should the page that
/// suspended it never resume it.
const SUSPEND_LIMIT: Duration = Duration::from_secs(60);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Table {
    keys: Vec<Key>,
    reserved: Vec<String>,
    task_manager: Vec<String>,
    editing: Vec<String>,
}

#[derive(Deserialize)]
struct Key {
    /// The W3C `code` name, as stored in the settings.
    code: String,
    vk: u16,
    group: String,
}

static TABLE: LazyLock<Table> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../ui/src/lib/hotkey-table.json"))
        .expect("hotkey-table.json")
});

/// A parsed combination: modifier bits and an index into the key table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Accelerator {
    mods: u32,
    key: usize,
}

impl Accelerator {
    fn key(&self) -> &'static Key {
        &TABLE.keys[self.key]
    }

    /// The stored form: `Ctrl+Alt+Shift+Super+<code>`, modifiers in that order.
    pub fn canonical(&self) -> String {
        let mut parts = Vec::new();
        for (bit, name) in [
            (CTRL, "Ctrl"),
            (ALT, "Alt"),
            (SHIFT, "Shift"),
            (WIN, "Super"),
        ] {
            if self.mods & bit != 0 {
                parts.push(name);
            }
        }
        parts.push(&self.key().code);
        parts.join("+")
    }

    /// Within the 0x0000–0xBFFF that RegisterHotKey leaves to applications.
    fn id(&self) -> i32 {
        1 + ((self.mods << 8) | u32::from(self.key().vk)) as i32
    }
}

/// Why a combination is refused; the page explains each.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Issue {
    /// Not of the form `Modifier+…+Key`.
    Invalid,
    /// Modifiers only.
    NoKey,
    /// Not in the key table, a media key, or Pause with Ctrl.
    Unsupported,
    LockKey,
    /// Neither Ctrl, Alt nor Win (F13–F24 and Shift+F1–F11 aside; Shift+F10
    /// is reserved).
    NeedsModifier,
    /// Used by Windows or by every program (Alt+F4, F12, ...).
    Reserved,
    /// Ctrl+Shift+Esc, which opens the task manager.
    TaskManager,
    /// Copy, paste and the like.
    Editing,
}

/// Reads a stored or hand-written combination: any case, a few aliases, and
/// `NumpadEnter` as `Enter` (the same key to Windows).
pub fn parse(text: &str) -> Result<Accelerator, Issue> {
    let tokens: Vec<&str> = text.split('+').map(str::trim).collect();
    if tokens.iter().any(|t| t.is_empty()) {
        return Err(Issue::Invalid);
    }
    let (last, rest) = tokens.split_last().ok_or(Issue::Invalid)?;
    let mut mods = 0;
    for token in rest {
        mods |= modifier(token).ok_or(Issue::Invalid)?;
    }
    if modifier(last).is_some() {
        return Err(Issue::NoKey);
    }
    let key = key_index(last).ok_or(Issue::Unsupported)?;
    Ok(Accelerator { mods, key })
}

fn modifier(token: &str) -> Option<u32> {
    match token.to_ascii_lowercase().as_str() {
        "ctrl" | "control" | "commandorcontrol" | "commandorctrl" | "cmdorctrl"
        | "cmdorcontrol" => Some(CTRL),
        "alt" | "option" => Some(ALT),
        "shift" => Some(SHIFT),
        "super" | "win" | "meta" | "cmd" | "command" => Some(WIN),
        _ => None,
    }
}

fn key_index(token: &str) -> Option<usize> {
    let lower = token.to_ascii_lowercase();
    let name = match lower.as_str() {
        "esc" => "Escape".to_string(),
        "numpadenter" => "Enter".to_string(),
        "up" | "down" | "left" | "right" => format!("Arrow{lower}"),
        s if s.len() == 1 && s.as_bytes()[0].is_ascii_lowercase() => format!("Key{s}"),
        s if s.len() == 1 && s.as_bytes()[0].is_ascii_digit() => format!("Digit{s}"),
        _ => lower,
    };
    TABLE
        .keys
        .iter()
        .position(|k| k.code.eq_ignore_ascii_case(&name))
}

/// The rules for a new combination. Saved ones are registered as they are,
/// so tightening these never takes a working hotkey away.
pub fn check(a: &Accelerator) -> Result<(), Issue> {
    let key = a.key();
    match key.group.as_str() {
        "media" => return Err(Issue::Unsupported),
        "lock" => return Err(Issue::LockKey),
        _ => {}
    }
    let name = a.canonical();
    if TABLE.reserved.contains(&name) {
        return Err(Issue::Reserved);
    }
    if TABLE.task_manager.contains(&name) {
        return Err(Issue::TaskManager);
    }
    if TABLE.editing.contains(&name) {
        return Err(Issue::Editing);
    }
    // With Ctrl down the keyboard sends Break, which Windows reports as
    // VK_CANCEL: a hotkey on Ctrl+…+VK_PAUSE would never fire.
    if key.code == "Pause" && a.mods & CTRL != 0 {
        return Err(Issue::Unsupported);
    }
    let shift_function = a.mods == SHIFT && key.group == "function";
    if a.mods & (CTRL | ALT | WIN) == 0 && key.group != "extra" && !shift_function {
        return Err(Issue::NeedsModifier);
    }
    Ok(())
}

/// Why the hotkey is not active, or why a change failed. The page words it;
/// `shortcut` is the stored form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum HotkeyError {
    Invalid {
        shortcut: String,
        issue: Issue,
    },
    /// Registered by Windows or another program (or another Argus).
    Taken {
        shortcut: String,
    },
    Failed {
        shortcut: String,
        code: u32,
    },
    SaveFailed,
}

impl HotkeyError {
    /// For the log: the kind of failure, never the combination.
    fn summary(&self) -> String {
        match self {
            HotkeyError::Invalid { issue, .. } => format!("invalid ({issue:?})"),
            HotkeyError::Taken { .. } => "taken".into(),
            HotkeyError::Failed { code, .. } => format!("os error {code}"),
            HotkeyError::SaveFailed => "settings not saved".into(),
        }
    }
}

enum RegisterError {
    Taken,
    Os(u32),
}

impl RegisterError {
    fn for_shortcut(self, a: &Accelerator) -> HotkeyError {
        let shortcut = a.canonical();
        match self {
            RegisterError::Taken => HotkeyError::Taken { shortcut },
            RegisterError::Os(code) => HotkeyError::Failed { shortcut, code },
        }
    }
}

/// Registers combinations with the system; tests use a fake.
trait Registrar {
    fn register(&mut self, a: &Accelerator) -> Result<(), RegisterError>;
    fn unregister(&mut self, a: &Accelerator);
}

/// What is registered now, which is not always what the settings say.
#[derive(Debug, Default)]
struct Hotkey {
    registered: Option<Accelerator>,
    /// Off while the settings page records a new combination, so that the
    /// current one reaches the page instead of hiding the window.
    suspended: bool,
    /// Bumped on every change, so a stale suspension timeout does nothing.
    generation: u64,
    error: Option<HotkeyError>,
}

impl Hotkey {
    fn release(&mut self, reg: &mut impl Registrar) {
        if let Some(a) = self.registered.take() {
            reg.unregister(&a);
        }
    }

    /// Registers the saved combination (at startup, after a suspension, on a
    /// retry). Returns whether anything changed.
    fn restore(&mut self, reg: &mut impl Registrar, saved: &str) -> bool {
        let before = (self.registered, self.suspended, self.error.clone());
        self.suspended = false;
        let target = if saved.is_empty() {
            Ok(None)
        } else {
            parse(saved).map(Some)
        };
        match target {
            Err(issue) => {
                self.release(reg);
                self.error = Some(HotkeyError::Invalid {
                    shortcut: saved.into(),
                    issue,
                });
            }
            Ok(target) if target == self.registered => self.error = None,
            Ok(target) => {
                self.release(reg);
                self.error = None;
                if let Some(a) = target {
                    match reg.register(&a) {
                        Ok(()) => self.registered = Some(a),
                        Err(e) => self.error = Some(e.for_shortcut(&a)),
                    }
                }
            }
        }
        self.generation += 1;
        before != (self.registered, self.suspended, self.error.clone())
    }

    fn suspend(&mut self, reg: &mut impl Registrar) {
        self.release(reg);
        self.suspended = true;
        self.generation += 1;
    }

    /// Checks, registers and saves `text` (empty turns the hotkey off). The
    /// new combination is registered before the old one goes, and dropped
    /// again if saving fails, so a failure leaves everything as it was.
    fn change(
        &mut self,
        reg: &mut impl Registrar,
        text: &str,
        save: impl FnOnce(&str) -> std::io::Result<()>,
    ) -> Result<(), HotkeyError> {
        let next = if text.is_empty() {
            None
        } else {
            let a = parse(text)
                .and_then(|a| check(&a).map(|()| a))
                .map_err(|issue| HotkeyError::Invalid {
                    shortcut: text.into(),
                    issue,
                })?;
            Some(a)
        };
        let fresh = next.filter(|a| self.registered != Some(*a));
        if let Some(a) = fresh {
            reg.register(&a).map_err(|e| e.for_shortcut(&a))?;
        }
        if save(&next.map_or_else(String::new, |a| a.canonical())).is_err() {
            if let Some(a) = fresh {
                reg.unregister(&a);
            }
            return Err(HotkeyError::SaveFailed);
        }
        if let Some(old) = self.registered
            && Some(old) != next
        {
            reg.unregister(&old);
        }
        self.registered = next;
        self.suspended = false;
        self.error = None;
        self.generation += 1;
        Ok(())
    }
}

#[derive(Default)]
pub struct HotkeyState(Mutex<Hotkey>);

/// RegisterHotKey on the main window, from the thread that owns it.
struct System;

static WINDOW: AtomicIsize = AtomicIsize::new(0);
static MAIN_THREAD: OnceLock<ThreadId> = OnceLock::new();
static APP: OnceLock<AppHandle> = OnceLock::new();

impl Registrar for System {
    fn register(&mut self, a: &Accelerator) -> Result<(), RegisterError> {
        debug_assert_eq!(MAIN_THREAD.get(), Some(&std::thread::current().id()));
        let hwnd = HWND(WINDOW.load(Ordering::Relaxed) as _);
        let mods = HOT_KEY_MODIFIERS(a.mods) | MOD_NOREPEAT;
        unsafe { RegisterHotKey(Some(hwnd), a.id(), mods, u32::from(a.key().vk)) }.map_err(|e| {
            let code = (e.code().0 as u32) & 0xFFFF;
            if code == ERROR_HOTKEY_ALREADY_REGISTERED.0 {
                RegisterError::Taken
            } else {
                RegisterError::Os(code)
            }
        })
    }

    fn unregister(&mut self, a: &Accelerator) {
        let hwnd = HWND(WINDOW.load(Ordering::Relaxed) as _);
        let _ = unsafe { UnregisterHotKey(Some(hwnd), a.id()) };
    }
}

const HOTKEY_SUBCLASS: usize = 1;

/// Takes WM_HOTKEY on the main window and registers the saved hotkey;
/// called once from setup, on the main thread.
pub fn init(app: &AppHandle, window: &WebviewWindow) {
    let Ok(hwnd) = window.hwnd() else { return };
    let hwnd = HWND(hwnd.0 as _);
    WINDOW.store(hwnd.0 as isize, Ordering::Relaxed);
    let _ = MAIN_THREAD.set(std::thread::current().id());
    let _ = APP.set(app.clone());
    unsafe {
        let _ = SetWindowSubclass(hwnd, Some(hotkey_proc), HOTKEY_SUBCLASS, 0);
    }
    let store = app.state::<SettingsStore>();
    let saved = store.get().hotkey;
    // A hand-written value reads the same from now on; it is written back
    // with the next change.
    if let Ok(a) = parse(&saved) {
        store.current.lock().unwrap().hotkey = a.canonical();
    }
    let state = app.state::<HotkeyState>();
    let mut hotkey = state.0.lock().unwrap();
    hotkey.restore(&mut System, &store.get().hotkey);
    if let Some(e) = &hotkey.error {
        log::warn!("hotkey not registered: {}", e.summary());
    }
}

unsafe extern "system" fn hotkey_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    // Negative ids are the system's own (snapshot hotkeys).
    if msg == WM_HOTKEY
        && (wparam.0 as isize) > 0
        && let Some(app) = APP.get()
    {
        crate::shell::toggle_main(app);
        return LRESULT(0);
    }
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

pub fn error<R: Runtime>(app: &AppHandle<R>) -> Option<HotkeyError> {
    app.state::<HotkeyState>().0.lock().unwrap().error.clone()
}

/// Replaces the hotkey with `text` and saves it; empty turns it off.
pub fn set<R: Runtime>(app: &AppHandle<R>, text: &str) -> Result<(), HotkeyError> {
    let store = app.state::<SettingsStore>();
    let state = app.state::<HotkeyState>();
    let result = state.0.lock().unwrap().change(&mut System, text, |value| {
        store.update(|s| s.hotkey = value.into()).map(|_| ())
    });
    if let Err(e) = &result {
        log::warn!("hotkey not changed: {}", e.summary());
    }
    result
}

/// Unregisters the hotkey while the page records a new one; it comes back
/// with [`resume`], when the window hides, or after [`SUSPEND_LIMIT`].
pub fn suspend<R: Runtime>(app: &AppHandle<R>) {
    let generation = {
        let state = app.state::<HotkeyState>();
        let mut hotkey = state.0.lock().unwrap();
        hotkey.suspend(&mut System);
        hotkey.generation
    };
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(SUSPEND_LIMIT);
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || {
            let stale = {
                let state = handle.state::<HotkeyState>();
                let hotkey = state.0.lock().unwrap();
                !hotkey.suspended || hotkey.generation != generation
            };
            if !stale && resume(&handle) {
                let _ = handle.emit(CHANGED_EVENT, ());
            }
        });
    });
}

/// Registers the saved hotkey again (ends a suspension, retries a failure).
/// Returns whether anything changed.
pub fn resume<R: Runtime>(app: &AppHandle<R>) -> bool {
    let saved = app.state::<SettingsStore>().get().hotkey;
    let state = app.state::<HotkeyState>();
    state.0.lock().unwrap().restore(&mut System, &saved)
}

/// Ends a suspension the page left behind when the window hides.
pub fn resume_if_suspended<R: Runtime>(app: &AppHandle<R>) {
    let suspended = app.state::<HotkeyState>().0.lock().unwrap().suspended;
    if suspended && resume(app) {
        let _ = app.emit(CHANGED_EVENT, ());
    }
}

/// Tries again a hotkey that was taken or failed, as the window shows or
/// comes to the front: the program holding it may have quit. Callers may be
/// on any thread.
pub fn retry_failed<R: Runtime>(app: &AppHandle<R>) {
    let failed = |app: &AppHandle<R>| {
        let state = app.state::<HotkeyState>();
        let hotkey = state.0.lock().unwrap();
        !hotkey.suspended
            && matches!(
                hotkey.error,
                Some(HotkeyError::Taken { .. } | HotkeyError::Failed { .. })
            )
    };
    if failed(app) {
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || {
            // Again on the main thread: a recording may have started since.
            if failed(&handle) && resume(&handle) {
                let _ = handle.emit(CHANGED_EVENT, ());
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Registrations kept in a list; `taken` lists combinations "another
    /// program" holds.
    #[derive(Default)]
    struct Fake {
        registered: Vec<String>,
        taken: Vec<String>,
    }

    impl Registrar for Fake {
        fn register(&mut self, a: &Accelerator) -> Result<(), RegisterError> {
            let name = a.canonical();
            if self.taken.contains(&name) || self.registered.contains(&name) {
                return Err(RegisterError::Taken);
            }
            self.registered.push(name);
            Ok(())
        }

        fn unregister(&mut self, a: &Accelerator) {
            let name = a.canonical();
            self.registered.retain(|r| *r != name);
        }
    }

    fn ok_save(_: &str) -> std::io::Result<()> {
        Ok(())
    }

    fn failing_save(_: &str) -> std::io::Result<()> {
        Err(std::io::Error::other("read-only"))
    }

    fn started(saved: &str) -> (Hotkey, Fake) {
        let mut hotkey = Hotkey::default();
        let mut fake = Fake::default();
        hotkey.restore(&mut fake, saved);
        (hotkey, fake)
    }

    #[test]
    fn parses_stored_and_hand_written_forms() {
        let canonical = |text: &str| parse(text).map(|a| a.canonical());
        assert_eq!(
            canonical(crate::settings::DEFAULT_HOTKEY).as_deref(),
            Ok("Ctrl+Alt+Escape")
        );
        assert_eq!(
            canonical("super+shift+alt+ctrl+k").as_deref(),
            Ok("Ctrl+Alt+Shift+Super+KeyK")
        );
        assert_eq!(canonical(" Control + Esc ").as_deref(), Ok("Ctrl+Escape"));
        assert_eq!(
            canonical("Ctrl+Alt+NumpadEnter").as_deref(),
            Ok("Ctrl+Alt+Enter")
        );
        assert_eq!(canonical("Win+Up").as_deref(), Ok("Super+ArrowUp"));
        assert_eq!(canonical("Ctrl+7").as_deref(), Ok("Ctrl+Digit7"));
        assert_eq!(canonical("Ctrl+"), Err(Issue::Invalid));
        assert_eq!(canonical("Ctrl+K+Alt"), Err(Issue::Invalid));
        assert_eq!(canonical("Ctrl+Alt"), Err(Issue::NoKey));
        assert_eq!(canonical("Shift"), Err(Issue::NoKey));
        assert_eq!(canonical("Ctrl+Alt+AltRight"), Err(Issue::Unsupported));
        assert_eq!(canonical("Ctrl+Alt+NumpadEqual"), Err(Issue::Unsupported));
    }

    #[test]
    fn default_hotkey_follows_the_rules() {
        let a = parse(crate::settings::DEFAULT_HOTKEY).unwrap();
        assert_eq!(check(&a), Ok(()));
    }

    #[test]
    fn key_table_is_consistent() {
        let mut codes = std::collections::HashSet::new();
        let mut vks = std::collections::HashSet::new();
        for key in &TABLE.keys {
            assert!(codes.insert(key.code.as_str()), "{} twice", key.code);
            assert!(vks.insert(key.vk), "{:#x} twice", key.vk);
        }
        for name in TABLE
            .reserved
            .iter()
            .chain(&TABLE.task_manager)
            .chain(&TABLE.editing)
        {
            let a = parse(name).unwrap_or_else(|_| panic!("{name}"));
            assert_eq!(&a.canonical(), name, "not in stored form");
        }
        let largest = (0..TABLE.keys.len())
            .map(|key| {
                let mods = CTRL | ALT | SHIFT | WIN;
                Accelerator { mods, key }.id()
            })
            .max()
            .unwrap();
        assert!(largest <= 0xBFFF);
    }

    /// The cases the settings page tests too: the page refuses what is
    /// `block`, and this side must refuse exactly those, for the same reason.
    #[test]
    fn rules_match_the_settings_page() {
        #[derive(Deserialize)]
        struct Case {
            hotkey: String,
            level: String,
            #[serde(default)]
            issue: Option<String>,
        }
        let cases: Vec<Case> =
            serde_json::from_str(include_str!("../../ui/tests/hotkey-cases.json")).unwrap();
        assert!(cases.len() > 20);
        for case in cases {
            let result = parse(&case.hotkey).and_then(|a| check(&a));
            match case.level.as_str() {
                "block" => {
                    let issue = result.expect_err(&case.hotkey);
                    let name = serde_json::to_value(issue).unwrap();
                    assert_eq!(name.as_str(), case.issue.as_deref(), "{}", case.hotkey);
                }
                _ => assert_eq!(result, Ok(()), "{}", case.hotkey),
            }
        }
    }

    #[test]
    fn errors_serialize_as_the_page_expects() {
        let taken = HotkeyError::Taken {
            shortcut: "Ctrl+Alt+KeyA".into(),
        };
        assert_eq!(
            serde_json::to_value(&taken).unwrap(),
            serde_json::json!({ "kind": "taken", "shortcut": "Ctrl+Alt+KeyA" })
        );
        let invalid = HotkeyError::Invalid {
            shortcut: "Shift+KeyA".into(),
            issue: Issue::NeedsModifier,
        };
        assert_eq!(
            serde_json::to_value(&invalid).unwrap(),
            serde_json::json!({ "kind": "invalid", "shortcut": "Shift+KeyA", "issue": "needsModifier" })
        );
        assert_eq!(
            serde_json::to_value(HotkeyError::SaveFailed).unwrap(),
            serde_json::json!({ "kind": "saveFailed" })
        );
    }

    #[test]
    fn startup_registers_saved_values_even_outside_the_rules() {
        let (hotkey, fake) = started("Shift+KeyA");
        assert_eq!(fake.registered, ["Shift+KeyA"]);
        assert_eq!(hotkey.error, None);

        let (hotkey, fake) = started("Ctrl+Alt+NumpadEqual");
        assert!(fake.registered.is_empty());
        assert!(matches!(
            hotkey.error,
            Some(HotkeyError::Invalid {
                issue: Issue::Unsupported,
                ..
            })
        ));

        let (hotkey, fake) = started("");
        assert!(fake.registered.is_empty());
        assert_eq!(hotkey.error, None);
    }

    #[test]
    fn startup_reports_a_taken_hotkey_and_a_retry_recovers() {
        let mut hotkey = Hotkey::default();
        let mut fake = Fake {
            taken: vec!["Ctrl+Alt+Escape".into()],
            ..Fake::default()
        };
        hotkey.restore(&mut fake, "Ctrl+Alt+Escape");
        assert!(matches!(hotkey.error, Some(HotkeyError::Taken { .. })));
        assert!(
            !hotkey.restore(&mut fake, "Ctrl+Alt+Escape"),
            "nothing changed"
        );

        fake.taken.clear();
        assert!(hotkey.restore(&mut fake, "Ctrl+Alt+Escape"));
        assert_eq!(hotkey.error, None);
        assert_eq!(fake.registered, ["Ctrl+Alt+Escape"]);
        assert!(
            !hotkey.restore(&mut fake, "Ctrl+Alt+Escape"),
            "already registered"
        );
        assert_eq!(fake.registered, ["Ctrl+Alt+Escape"]);
    }

    #[test]
    fn change_replaces_registers_and_saves() {
        let (mut hotkey, mut fake) = started("Ctrl+Alt+Escape");
        let mut saved = String::new();
        hotkey
            .change(&mut fake, "ctrl+alt+k", |v| {
                saved = v.into();
                Ok(())
            })
            .unwrap();
        assert_eq!(saved, "Ctrl+Alt+KeyK");
        assert_eq!(fake.registered, ["Ctrl+Alt+KeyK"]);

        // The same combination again: saved, not registered twice.
        hotkey.change(&mut fake, "Ctrl+Alt+KeyK", ok_save).unwrap();
        assert_eq!(fake.registered, ["Ctrl+Alt+KeyK"]);

        hotkey.change(&mut fake, "", ok_save).unwrap();
        assert!(fake.registered.is_empty());
        assert_eq!(hotkey.registered, None);
    }

    #[test]
    fn change_refuses_combinations_against_the_rules() {
        let (mut hotkey, mut fake) = started("Ctrl+Alt+Escape");
        let error = hotkey.change(&mut fake, "Shift+KeyA", ok_save).unwrap_err();
        assert_eq!(
            error,
            HotkeyError::Invalid {
                shortcut: "Shift+KeyA".into(),
                issue: Issue::NeedsModifier
            }
        );
        assert_eq!(fake.registered, ["Ctrl+Alt+Escape"]);
    }

    #[test]
    fn a_taken_combination_leaves_the_old_one() {
        let (mut hotkey, mut fake) = started("Ctrl+Alt+Escape");
        fake.taken.push("Ctrl+Alt+KeyA".into());
        let error = hotkey
            .change(&mut fake, "Ctrl+Alt+KeyA", ok_save)
            .unwrap_err();
        assert!(matches!(error, HotkeyError::Taken { .. }));
        assert_eq!(fake.registered, ["Ctrl+Alt+Escape"]);
        assert_eq!(hotkey.registered, parse("Ctrl+Alt+Escape").ok());
    }

    #[test]
    fn a_failed_save_leaves_no_second_hotkey() {
        let (mut hotkey, mut fake) = started("Ctrl+Alt+Escape");
        let error = hotkey
            .change(&mut fake, "Ctrl+Alt+KeyK", failing_save)
            .unwrap_err();
        assert_eq!(error, HotkeyError::SaveFailed);
        assert_eq!(fake.registered, ["Ctrl+Alt+Escape"]);
        assert_eq!(hotkey.registered, parse("Ctrl+Alt+Escape").ok());
    }

    #[test]
    fn suspension_frees_the_combination_until_resumed_or_changed() {
        let (mut hotkey, mut fake) = started("Ctrl+Alt+Escape");
        let generation = hotkey.generation;
        hotkey.suspend(&mut fake);
        assert!(fake.registered.is_empty());
        assert!(hotkey.generation > generation);

        // Recording the same combination while suspended registers it again.
        hotkey
            .change(&mut fake, "Ctrl+Alt+Escape", ok_save)
            .unwrap();
        assert_eq!(fake.registered, ["Ctrl+Alt+Escape"]);
        assert!(!hotkey.suspended);

        hotkey.suspend(&mut fake);
        fake.taken.push("Ctrl+Alt+KeyA".into());
        assert!(hotkey.change(&mut fake, "Ctrl+Alt+KeyA", ok_save).is_err());
        assert!(hotkey.suspended, "still recording");
        assert!(fake.registered.is_empty());

        assert!(hotkey.restore(&mut fake, "Ctrl+Alt+Escape"));
        assert_eq!(fake.registered, ["Ctrl+Alt+Escape"]);
        assert!(!hotkey.suspended);
    }

    #[test]
    fn a_failed_save_while_suspended_stays_suspended() {
        let (mut hotkey, mut fake) = started("Ctrl+Alt+Escape");
        hotkey.suspend(&mut fake);
        let error = hotkey
            .change(&mut fake, "Ctrl+Alt+KeyK", failing_save)
            .unwrap_err();
        assert_eq!(error, HotkeyError::SaveFailed);
        assert!(fake.registered.is_empty());
        assert!(hotkey.suspended);
    }
}
