//! Display-only process filtering. Collection and system totals remain complete.
//! The rules' editor is in the interface (`frontend/visibility.rs`).
use crate::model::{boolean, text};
use crate::prefs;
use serde_json::Value;
use std::collections::BTreeSet;

pub const RULES: &str = "processVisibility";
pub fn preference(page: &str) -> &'static str {
    if page == "network" {
        "hideSystem.network"
    } else {
        "hideSystem.processes"
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Rules {
    pub keep: BTreeSet<String>,
    pub hide: BTreeSet<String>,
}
impl Default for Rules {
    fn default() -> Self {
        Self {
            keep: [
                "explorer.exe",
                "cmd.exe",
                "conhost.exe",
                "openconsole.exe",
                "powershell.exe",
                "pwsh.exe",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
            hide: BTreeSet::new(),
        }
    }
}
impl Rules {
    pub fn load(prefs: &prefs::Prefs) -> Self {
        let saved: Self = prefs.get(RULES, Self::default());
        Self {
            keep: normalize(&saved.keep.into_iter().collect::<Vec<_>>().join(","))
                .unwrap_or_else(|_| Self::default().keep),
            hide: normalize(&saved.hide.into_iter().collect::<Vec<_>>().join(","))
                .unwrap_or_default(),
        }
    }
    pub fn hides(&self, name: &str, windows: bool) -> bool {
        let name = name.to_lowercase();
        !self.keep.contains(&name) && (windows || self.hide.contains(&name))
    }
    pub fn hides_process(&self, process: &Value) -> bool {
        self.hides(text(process, "name"), boolean(process, "windowsProcess"))
    }
}

/// Exact executable names, never paths or wildcard patterns. The editor
/// only needs to know that a list is invalid, not why.
#[allow(clippy::result_unit_err)]
pub fn normalize(value: &str) -> Result<BTreeSet<String>, ()> {
    let mut names = BTreeSet::new();
    if value.len() > 32_768 {
        return Err(());
    }
    for name in value
        .split([',', ';', '\n', '\r', '，', '；'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if name.len() > 260
            || name
                .chars()
                .any(|c| c.is_control() || "\\/:*?\"<>|".contains(c))
        {
            return Err(());
        }
        names.insert(name.to_lowercase());
        if names.len() > 128 {
            return Err(());
        }
    }
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::{Rules, normalize};
    #[test]
    fn windows_components_hide_but_interactive_tools_and_other_publishers_remain() {
        let rules = Rules::default();
        assert!(rules.hides("svchost.exe", true));
        assert!(rules.hides("System", true));
        for name in ["CMD.EXE", "conhost.exe", "explorer.exe", "powershell.exe"] {
            assert!(!rules.hides(name, true));
        }
        for name in ["msedge.exe", "code.exe", "svchost.exe"] {
            assert!(!rules.hides(name, false));
        }
    }
    #[test]
    fn exact_name_overrides_are_case_insensitive_and_keep_wins() {
        let rules = Rules {
            keep: normalize("CMD.exe;helper.exe").unwrap(),
            hide: normalize("HELPER.exe, worker.exe").unwrap(),
        };
        assert!(!rules.hides("helper.exe", false));
        assert!(rules.hides("WORKER.exe", false));
        assert!(!rules.hides("other-worker.exe", false));
        assert_eq!(normalize("a.exe，A.exe; b.exe\n").unwrap().len(), 2);
        for input in ["C:\\Windows\\app.exe", "*.exe", "a\0.exe", "x/y"] {
            assert!(normalize(input).is_err());
        }
    }
}
