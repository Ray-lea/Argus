//! Native preferences keep the original key names and preserve unrecognized data.
use serde_json::Value;
use std::{collections::BTreeMap, path::PathBuf};
pub struct Prefs {
    path: PathBuf,
    values: BTreeMap<String, Value>,
}
impl Prefs {
    pub fn load(path: PathBuf) -> Self {
        let values = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        Self { path, values }
    }
    pub fn get<T: serde::de::DeserializeOwned>(&self, key: &str, fallback: T) -> T {
        self.values
            .get(key)
            .cloned()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or(fallback)
    }
    pub fn set(&mut self, key: &str, value: impl serde::Serialize) {
        if let Ok(value) = serde_json::to_value(value) {
            if let Err(error) = argus_app::preferences::set(&self.path, key.into(), value.clone()) {
                log::warn!(
                    "native preferences write failed: {}",
                    argus_core::diagnostics::failure(&error.to_string())
                );
            } else {
                self.values.insert(key.into(), value);
            }
        }
    }
}
