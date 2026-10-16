//! User settings that the native side needs before the UI loads (language,
//! theme, hotkey, close behavior, autostart, process rules, history recording,
//! updates, onboarding), persisted as JSON in the app config dir.

use std::path::PathBuf;
use std::sync::Mutex;

use argus_core::i18n::Locale;
use argus_core::rules::ProcessRule;
use serde::{Deserialize, Serialize};

pub const DEFAULT_HOTKEY: &str = "Ctrl+Alt+Escape";

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
    /// Closing the window keeps Argus running in the tray.
    pub close_to_tray: bool,
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
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: Language::default(),
            theme: Theme::default(),
            onboarding: 0,
            close_to_tray: true,
            hotkey: DEFAULT_HOTKEY.into(),
            hotkey_enabled: true,
            rules: Vec::new(),
            history: true,
            kernel_sensors: false,
            update_check: true,
            update_channel: UpdateChannel::default(),
            update_route: UpdateRoute::default(),
            update_proxy: crate::updater::PROXY_PRESETS[0].into(),
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
pub struct SettingsView {
    #[serde(flatten)]
    pub settings: Settings,
    pub autostart: bool,
    /// The language in effect (`language` resolved).
    pub locale: Locale,
    /// Why the hotkey is not active (e.g. taken by another app).
    pub hotkey_error: Option<crate::hotkey::HotkeyError>,
    pub default_hotkey: &'static str,
}

pub struct SettingsStore {
    path: PathBuf,
    pub current: Mutex<Settings>,
}

impl SettingsStore {
    pub fn load(path: PathBuf) -> Self {
        let mut current: Settings = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
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
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&self.path, serde_json::to_vec_pretty(&next)?)?;
        *s = next;
        Ok(s.clone())
    }
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
}
