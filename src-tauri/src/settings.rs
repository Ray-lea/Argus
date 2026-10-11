//! User settings that the native side needs before the UI loads (hotkey,
//! close behavior, autostart), persisted as JSON in the app config dir.

use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

pub const DEFAULT_HOTKEY: &str = "Ctrl+Alt+Escape";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Closing the window keeps Argus running in the tray.
    pub close_to_tray: bool,
    /// Global shortcut that shows/hides the window; empty disables it.
    pub hotkey: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            close_to_tray: true,
            hotkey: DEFAULT_HOTKEY.into(),
        }
    }
}

/// What the settings page shows: stored settings plus live state.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    #[serde(flatten)]
    pub settings: Settings,
    pub autostart: bool,
    /// Why the hotkey is not active (e.g. taken by another app).
    pub hotkey_error: Option<String>,
}

pub struct SettingsStore {
    path: PathBuf,
    pub current: Mutex<Settings>,
    pub hotkey_error: Mutex<Option<String>>,
}

impl SettingsStore {
    pub fn load(path: PathBuf) -> Self {
        let current = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Self {
            path,
            current: Mutex::new(current),
            hotkey_error: Mutex::new(None),
        }
    }

    pub fn get(&self) -> Settings {
        self.current.lock().unwrap().clone()
    }

    pub fn update(&self, f: impl FnOnce(&mut Settings)) -> std::io::Result<Settings> {
        let mut s = self.current.lock().unwrap();
        f(&mut s);
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&self.path, serde_json::to_vec_pretty(&*s)?)?;
        Ok(s.clone())
    }
}
