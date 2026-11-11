//! Process column catalog, validated preferences, and the selection dialog.

use super::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

mod dialog;

pub(super) const PREFERENCE: &str = "processColumns";

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Format {
    Name,
    Text,
    Identifier,
    Count,
    Percent,
    Memory,
    AddressSpace,
    Rate,
    Network,
    Duration,
    Timestamp,
    Boolean,
}

#[derive(Clone, Copy)]
pub(super) struct Definition {
    pub key: &'static str,
    pub category: &'static str,
    pub format: Format,
    pub width: f32,
    pub aggregate: bool,
}

const fn column(
    key: &'static str,
    category: &'static str,
    format: Format,
    width: f32,
    aggregate: bool,
) -> Definition {
    Definition {
        key,
        category,
        format,
        width,
        aggregate,
    }
}

pub(super) const CATEGORIES: [&str; 6] = ["general", "cpu", "memory", "io", "network", "gpu"];

use Format::*;
pub(super) const DEFINITIONS: &[Definition] = &[
    column("name", "general", Name, 220., false),
    column("imageName", "general", Text, 180., false),
    column("pid", "general", Identifier, 72., false),
    column("ppid", "general", Identifier, 82., false),
    column("path", "general", Text, 300., false),
    column("description", "general", Text, 220., false),
    column("company", "general", Text, 200., false),
    column("version", "general", Text, 130., false),
    column("architecture", "general", Text, 110., false),
    column("elevated", "general", Boolean, 90., false),
    column("commandLine", "general", Text, 360., false),
    column("sessionId", "general", Identifier, 80., false),
    column("createTime", "general", Timestamp, 160., false),
    column("threads", "general", Count, 76., true),
    column("handles", "general", Count, 86., true),
    column("suspended", "general", Boolean, 90., false),
    column("efficiency", "general", Boolean, 100., false),
    column("cpu", "cpu", Percent, 70., true),
    column("cpuTime", "cpu", Duration, 100., true),
    column("cpuUserTime", "cpu", Duration, 115., true),
    column("cpuKernelTime", "cpu", Duration, 115., true),
    column("basePriority", "cpu", Identifier, 100., false),
    column("mem", "memory", Memory, 96., true),
    column("workingSet", "memory", Memory, 110., true),
    column("peakWorkingSet", "memory", Memory, 125., false),
    column("commit", "memory", Memory, 110., true),
    column("virtualSize", "memory", AddressSpace, 150., false),
    column("pageFaults", "memory", Count, 110., true),
    column("diskRead", "io", Rate, 104., true),
    column("diskWrite", "io", Rate, 104., true),
    column("io", "io", Rate, 92., true),
    column("ioRead", "io", Rate, 110., true),
    column("ioWrite", "io", Rate, 110., true),
    column("ioOther", "io", Rate, 110., true),
    column("ioReadTotal", "io", Memory, 130., true),
    column("ioWriteTotal", "io", Memory, 130., true),
    column("ioOtherTotal", "io", Memory, 130., true),
    column("net", "network", Network, 92., true),
    column("netReceived", "network", Network, 110., true),
    column("netSent", "network", Network, 110., true),
    column("gpu", "gpu", Percent, 70., true),
    column("gpuEngine", "gpu", Text, 140., false),
    column("gpuDedicated", "gpu", Memory, 130., true),
    column("gpuShared", "gpu", Memory, 130., true),
];

pub(super) const DEFAULTS: [&str; 11] = [
    "name",
    "pid",
    "cpu",
    "mem",
    "diskRead",
    "diskWrite",
    "io",
    "gpu",
    "net",
    "threads",
    "handles",
];

pub(super) fn definition(key: &str) -> Option<&'static Definition> {
    DEFINITIONS.iter().find(|column| column.key == key)
}

/// A text column's value as the table shows it, which is also what it sorts by.
pub(super) fn shown<'a>(key: &str, value: &'a str) -> std::borrow::Cow<'a, str> {
    match key {
        "architecture" => label(&format!("details.arch.{value}")).into(),
        _ => value.into(),
    }
}

pub(super) fn raw_key(key: &str) -> &str {
    match key {
        "imageName" => "name",
        "mem" => "privateWs",
        other => other,
    }
}

/// The columns of `category` ("all" for every one), or those matching every
/// word of `query` by key, name or description.
pub(super) fn matching(category: &str, query: &str) -> Vec<&'static Definition> {
    let terms = query
        .split_whitespace()
        .map(str::to_lowercase)
        .collect::<Vec<_>>();
    DEFINITIONS
        .iter()
        .filter(|column| {
            if terms.is_empty() {
                return category == "all" || column.category == category;
            }
            let text = format!(
                "{} {} {} {}",
                column.key,
                label(&format!("processes.column.{}.label", column.key)),
                label(&format!("processes.column.{}.title", column.key)),
                if column.key == "virtualSize" {
                    label("processes.columns.virtualAliases")
                } else {
                    String::new()
                }
            )
            .to_lowercase();
            let compact = text
                .chars()
                .filter(|c| !c.is_whitespace())
                .collect::<String>();
            terms
                .iter()
                .all(|term| text.contains(term) || compact.contains(term))
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(super) struct Preferences {
    pub selected: Vec<String>,
    /// All known columns retain a position, including temporarily hidden columns.
    pub order: Vec<String>,
    pub auto_width: bool,
    pub widths: BTreeMap<String, f32>,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            selected: DEFAULTS.iter().map(|s| (*s).into()).collect(),
            order: Vec::new(),
            auto_width: true,
            widths: BTreeMap::new(),
        }
    }
}

impl Preferences {
    pub(super) fn load(prefs: &prefs::Prefs) -> Self {
        Self::parse(&prefs.get(PREFERENCE, Value::Null))
    }

    pub(super) fn parse(value: &Value) -> Self {
        let mut preferences = serde_json::from_value::<Self>(value.clone()).unwrap_or_default();
        let mut seen = HashSet::new();
        preferences
            .selected
            .retain(|key| definition(key).is_some() && key != "name" && seen.insert(key.clone()));
        preferences.selected.insert(0, "name".into());
        preferences.normalize_order();
        preferences.widths.retain(|key, width| {
            definition(key).is_some() && width.is_finite() && (48. ..=1200.).contains(width)
        });
        preferences
    }

    pub(super) fn visible(&self, network: bool) -> Vec<&'static Definition> {
        self.ordered_selected()
            .into_iter()
            .filter_map(|key| definition(key))
            .filter(|column| network || column.category != "network")
            .collect()
    }

    pub(super) fn set_selected(&mut self, key: &str, enabled: bool) {
        if key == "name" || definition(key).is_none() {
            return;
        }
        self.normalize_order();
        if enabled {
            if !self.selected.iter().any(|saved| saved == key) {
                self.selected.push(key.into());
            }
        } else {
            self.selected.retain(|saved| saved != key);
        }
        self.normalize_order();
    }

    fn normalize_order(&mut self) {
        let mut seen = HashSet::new();
        self.order
            .retain(|key| key != "name" && definition(key).is_some() && seen.insert(key.clone()));
        self.order.insert(0, "name".into());
        seen.insert("name".into());
        for key in &self.selected {
            if seen.insert(key.clone()) {
                self.order.push(key.clone());
            }
        }
        for column in DEFINITIONS {
            if seen.insert(column.key.into()) {
                self.order.push(column.key.into());
            }
        }
        let selected: HashSet<_> = self.selected.iter().cloned().collect();
        self.selected = self
            .order
            .iter()
            .filter(|key| selected.contains(*key))
            .cloned()
            .collect();
    }

    pub(super) fn ordered_selected(&self) -> Vec<&str> {
        let order = if self.order.is_empty() {
            &self.selected
        } else {
            &self.order
        };
        order
            .iter()
            .filter(|key| self.selected.contains(key))
            .map(String::as_str)
            .collect()
    }

    pub(super) fn move_before(&mut self, key: &str, before: Option<&str>) -> bool {
        if key == "name"
            || !self.selected.iter().any(|value| value == key)
            || before.is_some_and(|target| {
                target == "name" || target == key || definition(target).is_none()
            })
        {
            return false;
        }
        self.normalize_order();
        let previous = self.order.clone();
        self.order.retain(|value| value != key);
        let index = before
            .and_then(|target| self.order.iter().position(|value| value == target))
            .unwrap_or(self.order.len());
        self.order.insert(index, key.into());
        self.normalize_order();
        self.order != previous
    }
}

#[cfg(test)]
mod tests {
    use super::{DEFAULTS, DEFINITIONS, Preferences};
    use serde_json::json;
    use std::collections::HashSet;

    #[test]
    fn selection_keeps_name_and_rejects_unknown_duplicate_and_invalid_widths() {
        let preferences = Preferences::parse(
            &json!({"selected":["gpu","unknown","pid","gpu","name"],"autoWidth":false,"widths":{"gpu":98,"pid":-1,"unknown":150,"name":2000}}),
        );
        assert_eq!(preferences.selected, ["name", "gpu", "pid"]);
        assert!(!preferences.auto_width);
        assert_eq!(preferences.widths.len(), 1);
        assert_eq!(preferences.widths["gpu"], 98.);
        assert_eq!(
            Preferences::parse(&json!({"selected":[]})).selected,
            ["name"]
        );
    }

    #[test]
    fn unavailable_network_columns_remain_selected_for_recovery() {
        let preferences = Preferences::default();
        assert!(preferences.auto_width);
        assert_eq!(preferences.visible(true).len(), DEFAULTS.len());
        assert_eq!(preferences.visible(false).len(), DEFAULTS.len() - 1);
        assert!(preferences.selected.iter().any(|key| key == "net"));
        assert_eq!(
            DEFINITIONS
                .iter()
                .map(|c| c.key)
                .collect::<HashSet<_>>()
                .len(),
            DEFINITIONS.len()
        );
    }

    #[test]
    fn order_migrates_and_survives_hiding_a_column_and_restart() {
        let mut prefs = Preferences::parse(&json!({"selected":["name","gpu","cpu","net","mem"]}));
        assert_eq!(
            prefs.ordered_selected(),
            ["name", "gpu", "cpu", "net", "mem"]
        );
        assert!(prefs.move_before("mem", Some("gpu")));
        assert_eq!(
            prefs.ordered_selected(),
            ["name", "mem", "gpu", "cpu", "net"]
        );
        prefs.set_selected("gpu", false);
        prefs.set_selected("gpu", true);
        let reloaded = Preferences::parse(&serde_json::to_value(&prefs).unwrap());
        assert_eq!(
            reloaded.ordered_selected(),
            ["name", "mem", "gpu", "cpu", "net"]
        );
        assert!(!prefs.move_before("name", None));
        assert!(!prefs.move_before("cpu", Some("name")));
        assert_eq!(
            prefs
                .visible(false)
                .iter()
                .map(|field| field.key)
                .collect::<Vec<_>>(),
            ["name", "mem", "gpu", "cpu"]
        );
    }

    #[test]
    fn every_selectable_column_has_entries_in_every_locale() {
        for locale in argus_core::i18n::Locale::ALL {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../locales")
                .join(locale.tag())
                .join("processes.json");
            let text = std::fs::read_to_string(path).unwrap();
            let catalog: serde_json::Value = serde_json::from_str(&text).unwrap();
            for column in DEFINITIONS {
                for key in ["label", "title"] {
                    assert!(
                        catalog["column"][column.key][key]
                            .as_str()
                            .is_some_and(|s| !s.is_empty()),
                        "{} {key}",
                        column.key
                    );
                }
            }
        }
    }
}
