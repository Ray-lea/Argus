//! Rules that give programs a priority or efficiency mode as they start: the
//! engine checks every new process instance against the rules once, and every
//! running one again whenever the rules change.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::actions::{self, ActionError, Priority};
use crate::process::ProcessRow;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessRule {
    pub id: String,
    /// Executable file name, matched case-insensitively (`chrome.exe`).
    pub name: String,
    pub enabled: bool,
    pub priority: Option<Priority>,
    /// `Some(true)` turns efficiency mode on, `Some(false)` off.
    pub efficiency: Option<bool>,
}

/// What a rule has done since Argus started (or since it was last edited).
#[derive(Debug, Clone, Copy, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleStats {
    pub applied: u32,
    /// Processes it could not change without administrator rights.
    pub denied: u32,
}

#[derive(Default)]
pub struct RuleEngine {
    rules: Vec<ProcessRule>,
    /// Active rule (index into `rules`) by lowercase executable name.
    by_name: HashMap<String, usize>,
    /// Process instances already checked.
    seen: HashSet<(u32, i64)>,
    stats: HashMap<String, RuleStats>,
    /// Check every process on the next call, not just new ones.
    recheck: bool,
}

impl RuleEngine {
    pub fn set_rules(&mut self, rules: Vec<ProcessRule>) {
        self.by_name.clear();
        for (i, r) in rules.iter().enumerate() {
            if r.enabled && (r.priority.is_some() || r.efficiency.is_some()) {
                // The first rule for a name wins.
                self.by_name
                    .entry(r.name.trim().to_lowercase())
                    .or_insert(i);
            }
        }
        // An edited rule starts counting again.
        let old: HashMap<&str, &ProcessRule> =
            self.rules.iter().map(|r| (r.id.as_str(), r)).collect();
        self.stats.retain(|id, _| {
            rules.iter().any(|r| {
                &r.id == id
                    && old.get(id.as_str()).is_some_and(|o| {
                        o.name == r.name
                            && o.enabled == r.enabled
                            && o.priority == r.priority
                            && o.efficiency == r.efficiency
                    })
            })
        });
        self.rules = rules;
        self.recheck = true;
    }

    /// Applies the rules to processes that started since the last call (to all
    /// of them right after the rules changed).
    pub fn apply(&mut self, processes: &[ProcessRow]) {
        if self.by_name.is_empty() {
            // Nothing to do; adding a rule rechecks everything anyway.
            self.seen.clear();
            self.recheck = false;
            return;
        }
        let mut seen = HashSet::with_capacity(processes.len());
        let mut name = String::new();
        for p in processes {
            let key = (p.pid, p.create_time);
            seen.insert(key);
            if !self.recheck && self.seen.contains(&key) {
                continue;
            }
            name.clear();
            name.extend(p.name.chars().flat_map(char::to_lowercase));
            let Some(&i) = self.by_name.get(&name) else {
                continue;
            };
            let rule = &self.rules[i];
            let result = apply_rule(rule, p);
            let stats = self.stats.entry(rule.id.clone()).or_default();
            match result {
                Ok(()) => stats.applied += 1,
                Err(ActionError::AccessDenied) => stats.denied += 1,
                // Exited meanwhile, or something unexpected: nothing to report.
                Err(_) => {}
            }
        }
        self.seen = seen;
        self.recheck = false;
    }

    pub fn stats(&self) -> HashMap<String, RuleStats> {
        self.stats.clone()
    }
}

fn apply_rule(rule: &ProcessRule, p: &ProcessRow) -> Result<(), ActionError> {
    // Efficiency mode first: turning it on also lowers the priority, which an
    // explicit priority in the same rule then overrides.
    if let Some(on) = rule.efficiency {
        actions::set_efficiency_mode(p.pid, p.create_time, on)?;
    }
    if let Some(priority) = rule.priority {
        actions::set_priority(p.pid, p.create_time, priority)?;
    }
    Ok(())
}
