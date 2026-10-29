//! User settings that the native side needs before the UI loads (language,
//! theme, hotkey, close behavior, autostart, process rules, history recording,
//! updates and what a new version shows first, onboarding), persisted as JSON
//! in the app config dir.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use argus_core::i18n::Locale;
use argus_core::rules::ProcessRule;
use serde::{Deserialize, Serialize};

pub const DEFAULT_HOTKEY: &str = "Ctrl+Alt+Escape";
/// The cadences on offer while minimized or in the tray, ms (Settings and the
/// onboarding offer the same list).
pub const BACKGROUND_INTERVALS: [u64; 6] = [500, 1000, 2000, 5000, 10_000, 30_000];
pub const DEFAULT_BACKGROUND_INTERVAL: u64 = 5000;

/// The UI language: a locale, or the Windows display language. Stored as the
/// tag; one this version does not know (from a newer Argus) reads as `System`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum Language {
    #[default]
    System,
    Locale(Locale),
}

impl Language {
    pub fn resolve(self) -> Locale {
        match self {
            Language::System => Locale::system(),
            Language::Locale(locale) => locale,
        }
    }
}

impl From<String> for Language {
    fn from(tag: String) -> Self {
        Locale::from_tag(&tag).map_or(Language::System, Language::Locale)
    }
}

impl From<Language> for String {
    fn from(language: Language) -> Self {
        match language {
            Language::System => "system".into(),
            Language::Locale(locale) => locale.tag().into(),
        }
    }
}

/// The color theme: light, dark, or the Windows app mode. Stored as its name;
/// one this version does not know reads as `System`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

impl From<String> for Theme {
    fn from(name: String) -> Self {
        match name.as_str() {
            "light" => Theme::Light,
            "dark" => Theme::Dark,
            _ => Theme::System,
        }
    }
}

impl From<Theme> for String {
    fn from(theme: Theme) -> Self {
        match theme {
            Theme::System => "system",
            Theme::Light => "light",
            Theme::Dark => "dark",
        }
        .into()
    }
}

/// Which releases count as updates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UpdateChannel {
    #[default]
    Stable,
    /// Prereleases as well as stable releases.
    Beta,
}

/// How update requests reach GitHub.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UpdateRoute {
    /// GitHub first, then the proxies.
    #[default]
    Auto,
    Direct,
    /// Only the chosen proxy.
    Proxy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub language: Language,
    pub theme: Theme,
    /// The onboarding version last completed or skipped; 0 before the first.
    /// The UI owns the numbering (`ONBOARDING_VERSION` in Onboarding.tsx).
    pub onboarding: u32,
    /// The version whose first start has been dealt with: its changes shown
    /// or announced, or nothing to show (a fresh install). Empty until a
    /// version that records it has run.
    pub seen_version: String,
    /// Closing the window keeps Argus running in the tray.
    pub close_to_tray: bool,
    /// How often to sample while the window is minimized, ms; one of
    /// `BACKGROUND_INTERVALS`.
    pub minimized_interval: u64,
    /// How often to sample while Argus is in the tray, ms.
    pub tray_interval: u64,
    /// Global shortcut that shows/hides the window, e.g. `Ctrl+Alt+Escape`
    /// (see hotkey.rs); empty disables it.
    #[serde(deserialize_with = "lenient_hotkey")]
    pub hotkey: String,
    /// Whether `hotkey` is registered; turning it off keeps the combination.
    pub hotkey_enabled: bool,
    /// Priority and efficiency mode rules, applied as programs start.
    pub rules: Vec<ProcessRule>,
    /// Record history for the history page.
    pub history: bool,
    /// Opt-in CPU sensors; an explicit enable action prepares PawnIO if needed.
    pub kernel_sensors: bool,
    /// Check for updates in the background.
    pub update_check: bool,
    pub update_channel: UpdateChannel,
    pub update_route: UpdateRoute,
    /// GitHub proxy prefix, e.g. `https://gh-proxy.com/`.
    pub update_proxy: String,
    /// Show what a new version changed on its first start; off, a notice
    /// says it was updated instead.
    pub whats_new: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: Language::default(),
            theme: Theme::default(),
            onboarding: 0,
            seen_version: String::new(),
            close_to_tray: true,
            minimized_interval: DEFAULT_BACKGROUND_INTERVAL,
            tray_interval: DEFAULT_BACKGROUND_INTERVAL,
            hotkey: DEFAULT_HOTKEY.into(),
            hotkey_enabled: true,
            rules: Vec::new(),
            history: true,
            kernel_sensors: false,
            update_check: true,
            update_channel: UpdateChannel::default(),
            update_route: UpdateRoute::default(),
            update_proxy: crate::config::PROXY_PRESETS[0].into(),
            whats_new: true,
        }
    }
}

impl Settings {
    /// Brings settings saved by an older version up to date, in memory; they
    /// are written back with the next change.
    fn migrate(&mut self) {
        // The hotkey used to be turned off by clearing it.
        if self.hotkey.is_empty() {
            self.hotkey = DEFAULT_HOTKEY.into();
            self.hotkey_enabled = false;
        }
        // A hand edit to an interval on offer nowhere.
        for ms in [&mut self.minimized_interval, &mut self.tray_interval] {
            if !BACKGROUND_INTERVALS.contains(ms) {
                *ms = DEFAULT_BACKGROUND_INTERVAL;
            }
        }
    }

    /// The sampler's cadences while minimized and while in the tray.
    pub fn background_intervals(&self) -> crate::sampler::Control {
        crate::sampler::Control::SetBackgroundIntervals {
            minimized: Duration::from_millis(self.minimized_interval),
            tray: Duration::from_millis(self.tray_interval),
        }
    }
}

/// A hotkey that is not text (a hand edit) reads as the default instead of
/// sending every other setting back to its default too.
fn lenient_hotkey<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    Ok(match serde_json::Value::deserialize(d)? {
        serde_json::Value::String(hotkey) => hotkey,
        _ => DEFAULT_HOTKEY.into(),
    })
}

/// What the settings page shows: stored settings plus live state.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView<E> {
    #[serde(flatten)]
    pub settings: Settings,
    pub autostart: bool,
    /// The language in effect (`language` resolved).
    pub locale: Locale,
    /// Why the hotkey is not active (e.g. taken by another app).
    pub hotkey_error: Option<E>,
    pub default_hotkey: &'static str,
}

pub struct SettingsStore {
    path: PathBuf,
    pub current: Mutex<Settings>,
}

impl SettingsStore {
    pub fn load(path: PathBuf) -> Self {
        let mut current = read(&path).unwrap_or_default();
        current.migrate();
        Self {
            path,
            current: Mutex::new(current),
        }
    }

    pub fn get(&self) -> Settings {
        self.current.lock().unwrap().clone()
    }

    pub fn update(&self, f: impl FnOnce(&mut Settings)) -> std::io::Result<Settings> {
        let mut s = self.current.lock().unwrap();
        let mut next = s.clone();
        f(&mut next);
        write(&self.path, &serde_json::to_vec_pretty(&next)?)?;
        *s = next;
        Ok(s.clone())
    }
}

/// The saved settings; None means defaults. Whatever stands in the way is
/// logged by kind, never by content: settings lost across a restart can then
/// be told apart (no file, an empty or zeroed one, one that does not parse).
/// A file that does not parse is kept as `settings.json.bad`, since the next
/// change would overwrite it.
fn read(path: &Path) -> Option<Settings> {
    let mut retries = 3;
    let bytes = loop {
        match std::fs::read(path) {
            Ok(bytes) => break bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                log::info!("settings: none saved, using defaults");
                return None;
            }
            // Another program (a virus scanner, say) may hold it for a moment;
            // defaults now would overwrite it with the next change.
            Err(_) if retries > 0 => {
                retries -= 1;
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => {
                log::warn!(
                    "settings: not read ({}), path={}, using defaults",
                    argus_core::diagnostics::io(&e),
                    argus_core::diagnostics::path(path)
                );
                return None;
            }
        }
    };
    match serde_json::from_slice::<Settings>(&bytes) {
        Ok(settings) => {
            log::info!("settings: loaded (onboarding {})", settings.onboarding);
            Some(settings)
        }
        Err(e) => {
            let kind = if bytes.is_empty() {
                "empty"
            } else if bytes.iter().all(|&b| b == 0) {
                "all zero bytes"
            } else {
                "not valid"
            };
            log::warn!(
                "settings: {} bytes, {kind} ({:?} error at line {}, column {}), path={}, using defaults",
                bytes.len(),
                e.classify(),
                e.line(),
                e.column(),
                argus_core::diagnostics::path(path)
            );
            if let Err(error) = std::fs::rename(path, path.with_extension("json.bad")) {
                argus_core::diagnostics::warn(
                    "preserve invalid settings",
                    format!(
                        "{}; source={}; destination={}",
                        argus_core::diagnostics::io(&error),
                        argus_core::diagnostics::path(path),
                        argus_core::diagnostics::path(&path.with_extension("json.bad"))
                    ),
                );
            }
            None
        }
    }
}

/// Writes through a temporary file that reaches the disk before it replaces
/// the old one, so a crash, a forced restart or a power cut leaves the old
/// settings or the new ones, never an empty or cut-off file.
pub(crate) fn write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut stage = "create settings directory";
    let result = write_with_stage(path, bytes, &mut stage);
    if let Err(error) = &result {
        argus_core::diagnostics::warn(
            stage,
            format!(
                "{}; path={}",
                argus_core::diagnostics::io(error),
                argus_core::diagnostics::path(path)
            ),
        );
    }
    result
}

fn write_with_stage(path: &Path, bytes: &[u8], stage: &mut &'static str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let partial = path.with_extension("json.partial");
    *stage = "create temporary settings file";
    let result = std::fs::File::create(&partial)
        .and_then(|mut file| {
            *stage = "write temporary settings file";
            file.write_all(bytes)?;
            *stage = "flush settings file";
            file.sync_all()
        })
        .and_then(|_| {
            *stage = "replace settings file";
            std::fs::rename(&partial, path)
        });
    if result.is_err() {
        let _ = std::fs::remove_file(&partial);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_save_does_not_enable_kernel_sensors_in_memory() {
        let id = unsafe { windows::Win32::System::Com::CoCreateGuid() }.unwrap();
        let directory = std::env::temp_dir().join(format!("argus-settings-{id:?}"));
        std::fs::create_dir(&directory).unwrap();
        let store = SettingsStore {
            path: directory.clone(),
            current: Mutex::new(Settings::default()),
        };
        assert!(
            store
                .update(|settings| settings.kernel_sensors = true)
                .is_err()
        );
        assert!(!store.get().kernel_sensors);
        std::fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn a_cleared_hotkey_reads_as_turned_off() {
        let mut settings: Settings = serde_json::from_str(r#"{"hotkey": ""}"#).unwrap();
        settings.migrate();
        assert_eq!(settings.hotkey, DEFAULT_HOTKEY);
        assert!(!settings.hotkey_enabled);

        let mut settings: Settings =
            serde_json::from_str(r#"{"hotkey": "Ctrl+Alt+KeyK"}"#).unwrap();
        settings.migrate();
        assert_eq!(settings.hotkey, "Ctrl+Alt+KeyK");
        assert!(settings.hotkey_enabled);
    }

    #[test]
    fn a_hotkey_that_is_not_text_keeps_the_other_settings() {
        let settings: Settings =
            serde_json::from_str(r#"{"hotkey": 5, "closeToTray": false}"#).unwrap();
        assert_eq!(settings.hotkey, DEFAULT_HOTKEY);
        assert!(!settings.close_to_tray);
    }

    #[test]
    fn background_intervals_default_to_5_s_and_drop_odd_values() {
        // Settings from before these existed.
        let mut old: Settings = serde_json::from_str(r#"{"closeToTray": true}"#).unwrap();
        old.migrate();
        assert_eq!((old.minimized_interval, old.tray_interval), (5000, 5000));
        let mut edited: Settings =
            serde_json::from_str(r#"{"minimizedInterval": 3000, "trayInterval": 30000}"#).unwrap();
        edited.migrate();
        assert_eq!(
            (edited.minimized_interval, edited.tray_interval),
            (5000, 30_000)
        );
    }

    #[test]
    fn what_changed_shows_by_default_and_old_settings_have_seen_no_version() {
        let old: Settings =
            serde_json::from_str(r#"{"closeToTray": true, "onboarding": 1}"#).unwrap();
        assert!(old.whats_new);
        assert_eq!(old.seen_version, "");
        let saved: Settings =
            serde_json::from_str(r#"{"whatsNew": false, "seenVersion": "0.2.0"}"#).unwrap();
        assert!(!saved.whats_new);
        assert_eq!(saved.seen_version, "0.2.0");
    }

    fn scratch() -> PathBuf {
        let id = unsafe { windows::Win32::System::Com::CoCreateGuid() }.unwrap();
        let directory = std::env::temp_dir().join(format!("argus-settings-{id:?}"));
        std::fs::create_dir(&directory).unwrap();
        directory
    }

    #[test]
    fn saved_settings_come_back_and_leave_no_temporary_file() {
        let directory = scratch();
        let path = directory.join("settings.json");
        let store = SettingsStore::load(path.clone());
        store.update(|s| s.onboarding = 1).unwrap();
        store
            .update(|s| s.language = Language::Locale(Locale::EnUs))
            .unwrap();
        let again = SettingsStore::load(path).get();
        assert_eq!(again.onboarding, 1);
        assert_eq!(again.language, Language::Locale(Locale::EnUs));
        let names: Vec<String> = std::fs::read_dir(&directory)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(names, vec!["settings.json"]);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_damaged_file_reads_as_defaults_and_is_kept_aside() {
        let directory = scratch();
        let path = directory.join("settings.json");
        let cut_off = b"{\"onboarding\": 1, \"lang";
        for damaged in [&b""[..], &[0; 300][..], &cut_off[..]] {
            std::fs::write(&path, damaged).unwrap();
            let store = SettingsStore::load(path.clone());
            assert_eq!(store.get().onboarding, 0);
            assert!(!path.exists());
            assert_eq!(
                std::fs::read(directory.join("settings.json.bad")).unwrap(),
                damaged
            );
            // The next change starts a new file and leaves the damaged one be.
            store.update(|s| s.onboarding = 1).unwrap();
            assert_eq!(SettingsStore::load(path.clone()).get().onboarding, 1);
            std::fs::remove_file(&path).unwrap();
        }
        std::fs::remove_dir_all(directory).unwrap();
    }
}
