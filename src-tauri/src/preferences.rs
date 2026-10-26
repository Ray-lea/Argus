//! UI preferences live outside the disposable WebView profile. Each change
//! replaces the file atomically; legacy localStorage values only fill gaps.

use std::{path::PathBuf, sync::Mutex};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tauri::{Manager, Runtime};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowPlacement {
    pub rect: [i32; 4],
    pub maximized: bool,
}

impl WindowPlacement {
    pub fn valid(self) -> bool {
        let [left, top, right, bottom] = self.rect;
        self.rect.iter().all(|v| v.abs_diff(0) <= 100_000)
            && (100..=32_768).contains(&(i64::from(right) - i64::from(left)))
            && (100..=32_768).contains(&(i64::from(bottom) - i64::from(top)))
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct Saved {
    values: Map<String, Value>,
    window: Option<WindowPlacement>,
}

pub struct Preferences {
    path: PathBuf,
    saved: Mutex<Saved>,
}

impl Preferences {
    pub fn load(path: PathBuf) -> Self {
        let saved = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|_| {
                let _ = std::fs::copy(&path, path.with_extension("json.bad"));
                argus_core::diagnostics::warn("read UI preferences", "invalid JSON preserved");
                Saved::default()
            }),
            Err(error) => {
                if error.kind() != std::io::ErrorKind::NotFound {
                    argus_core::diagnostics::warn(
                        "read UI preferences",
                        argus_core::diagnostics::io(&error),
                    );
                }
                Saved::default()
            }
        };
        Self {
            path,
            saved: Mutex::new(saved),
        }
    }

    pub fn values(&self) -> Map<String, Value> {
        self.saved.lock().unwrap().values.clone()
    }

    pub fn window(&self) -> Option<WindowPlacement> {
        self.saved.lock().unwrap().window.filter(|p| p.valid())
    }

    fn update(&self, change: impl FnOnce(&mut Saved)) -> std::io::Result<()> {
        let mut saved = self.saved.lock().unwrap();
        let mut next = saved.clone();
        change(&mut next);
        crate::settings::write(&self.path, &serde_json::to_vec(&next)?)?;
        *saved = next;
        Ok(())
    }

    pub fn save_window(&self, placement: WindowPlacement) {
        if placement.valid() && self.window() != Some(placement) {
            let _ = self.update(|s| s.window = Some(placement));
        }
    }
}

fn valid_values(values: &Map<String, Value>) -> bool {
    values.keys().all(|key| !key.is_empty() && key.len() <= 128)
        && serde_json::to_vec(values).is_ok_and(|bytes| bytes.len() <= 1024 * 1024)
}

pub fn zoom(values: &Map<String, Value>) -> f64 {
    values
        .get("uiScale")
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite() && v.fract() == 0.0 && (80.0..=150.0).contains(v))
        .unwrap_or(100.0)
        / 100.0
}

pub fn apply_zoom<R: Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.set_zoom(zoom(&app.state::<Preferences>().values()));
    }
}

#[tauri::command]
pub fn load_ui_preferences(
    app: tauri::AppHandle,
    legacy: Map<String, Value>,
) -> Result<Map<String, Value>, String> {
    let store = app.state::<Preferences>();
    if !valid_values(&legacy) {
        return Err(argus_core::tr!("host.savePreferencesFailed"));
    }
    let current = store.values();
    if legacy.keys().any(|k| !current.contains_key(k)) {
        store
            .update(|s| {
                for (key, value) in legacy {
                    s.values.entry(key).or_insert(value);
                }
            })
            .map_err(|_| argus_core::tr!("host.savePreferencesFailed"))?;
    }
    apply_zoom(&app);
    Ok(store.values())
}

#[tauri::command]
pub fn save_ui_preference(app: tauri::AppHandle, key: String, value: Value) -> Result<(), String> {
    let values = Map::from_iter([(key.clone(), value.clone())]);
    if !valid_values(&values) {
        return Err(argus_core::tr!("host.savePreferencesFailed"));
    }
    app.state::<Preferences>()
        .update(|s| {
            s.values.insert(key.clone(), value);
        })
        .map_err(|_| argus_core::tr!("host.savePreferencesFailed"))?;
    if key == "uiScale" {
        apply_zoom(&app);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometry_rejects_corruption_and_preserves_negative_monitor_coordinates() {
        assert!(
            WindowPlacement {
                rect: [-1600, 30, -300, 800],
                maximized: true
            }
            .valid()
        );
        assert!(
            !WindowPlacement {
                rect: [i32::MIN, 0, i32::MAX, 800],
                maximized: false
            }
            .valid()
        );
        assert!(
            !WindowPlacement {
                rect: [0, 0, 0, 800],
                maximized: false
            }
            .valid()
        );
    }

    #[test]
    fn preferences_and_normal_bounds_survive_restart_and_independent_updates() {
        let dir = std::env::temp_dir().join(format!(
            "argus-prefs-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = dir.join("preferences.json");
        let store = Preferences::load(path.clone());
        let rect = WindowPlacement {
            rect: [-1500, 40, -200, 850],
            maximized: true,
        };
        store.save_window(rect);
        store
            .update(|s| {
                s.values
                    .insert("sort".into(), serde_json::json!({"key":"mem","desc":false}));
            })
            .unwrap();
        let restored = Preferences::load(path);
        assert_eq!(restored.window(), Some(rect));
        assert_eq!(restored.values()["sort"]["key"], "mem");
        assert_eq!(
            zoom(&Map::from_iter([("uiScale".into(), Value::from(125))])),
            1.25
        );
        assert_eq!(
            zoom(&Map::from_iter([("uiScale".into(), Value::from(999))])),
            1.0
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
