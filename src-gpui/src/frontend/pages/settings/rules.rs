//! Rules: the priority and efficiency mode given to processes by name, and
//! how often each rule was applied, denied or failed.
use super::*;
use crate::frontend::widgets::{ButtonExt, Field, Tone};

const RULE_PRIORITIES: [&str; 6] = [
    "realtime",
    "high",
    "aboveNormal",
    "normal",
    "belowNormal",
    "idle",
];

impl Desktop {
    // ---- Rules ------------------------------------------------------------------

    fn add_rule(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(input) = self.ui.inputs.get("ruleDraft").cloned() else {
            return;
        };
        let name = input.read(cx).value().trim().to_owned();
        if name.is_empty() {
            return;
        }
        let mut rules = values(&self.data["settings"]["rules"]).to_vec();
        if rules
            .iter()
            .any(|r| model::same_executable_name(text(r, "name"), &name))
        {
            self.ui.errors.insert(
                "ruleDraft".into(),
                tr!("settings.rules.exists", name = name),
            );
            cx.notify();
            return;
        }
        let id = format!(
            "native-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        );
        rules.push(json!({"id":id,"name":name,"enabled":true,"priority":null,"efficiency":true}));
        self.request(
            "set_rules",
            json!({"rules":rules}),
            Some("settings"),
            window,
            cx,
        );
        input.update(cx, |state, cx| state.set_value("", window, cx));
        self.ui.errors.remove("ruleDraft");
        self.ui.focus_rule = Some((id, Instant::now()));
        cx.notify();
    }

    fn update_rule(&mut self, id: &str, patch: Value, window: &mut Window, cx: &mut Context<Self>) {
        let mut rules = values(&self.data["settings"]["rules"]).to_vec();
        for rule in &mut rules {
            if rule["id"] == id {
                for (key, value) in patch.as_object().into_iter().flatten() {
                    rule[key] = value.clone();
                }
            }
        }
        self.request(
            "set_rules",
            json!({"rules":rules}),
            Some("settings"),
            window,
            cx,
        );
    }

    pub(super) fn rule_cards(
        &mut self,
        settings: &Value,
        cards: &mut Vec<AnyElement>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let p = self.palette;
        let input = self.inline_input(
            "ruleDraft",
            "",
            |this, window, cx| this.add_rule(window, cx),
            window,
            cx,
        );
        let draft = input.read(cx).value().trim().to_owned();
        let add = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(div().w(px(220.)).child(self.text_field(
                &input,
                widgets::input(&input),
                Field::Boxed,
                label("settings.rules.placeholder").into(),
                true,
                window,
                cx,
            )))
            .child(
                self.btn("rule-add", label("settings.rules.add"), None, Tone::Plain)
                    .inert(draft.is_empty())
                    .on_click(cx.listener(|this, _, window, cx| this.add_rule(window, cx))),
            )
            .into_any_element();
        cards.push(self.setting(
            "rules",
            self.glyph("BoltIcon"),
            label("settings.rules.title"),
            Some(label("settings.rules.desc").into_any_element()),
            self.ui.errors.get("ruleDraft").cloned(),
            vec![add],
        ));
        let stats = self.data.get("ruleStats").cloned().unwrap_or(Value::Null);
        for rule in values(&settings["rules"]).to_vec() {
            let id = text(&rule, "id").to_owned();
            let name = text(&rule, "name").to_owned();
            let enabled = boolean(&rule, "enabled");
            let process = values(&self.snapshot["processes"])
                .iter()
                .find(|q| model::same_executable_name(text(q, "name"), &name))
                .cloned();
            let stat = stats.get(&id).cloned().or_else(|| {
                values(&stats)
                    .iter()
                    .find(|s| text(s, "id") == id || text(s, "name") == name)
                    .cloned()
            });
            let summary = if rule["priority"].is_null() && rule["efficiency"].is_null() {
                label("settings.rules.summary.noAction")
            } else if !enabled {
                label("settings.rules.summary.disabled")
            } else {
                let count = |key: &str| stat.as_ref().and_then(|s| s[key].as_u64()).unwrap_or(0);
                let (applied, denied, failed) =
                    (count("applied"), count("denied"), count("failed"));
                if applied + denied + failed == 0 {
                    label("settings.rules.summary.notSeen")
                } else {
                    let mut parts = vec![];
                    if applied > 0 {
                        parts.push(tr!("settings.rules.summary.applied", count = applied));
                    }
                    if denied > 0 {
                        parts.push(tr!("settings.rules.summary.denied", count = denied));
                    }
                    if failed > 0 {
                        parts.push(tr!("settings.rules.summary.failed", count = failed));
                    }
                    tr!(
                        "settings.rules.summary.sentence",
                        parts = parts.join(&label("settings.rules.summary.separator"))
                    )
                }
            };
            let priority_values = std::iter::once("")
                .chain(RULE_PRIORITIES)
                .collect::<Vec<_>>();
            let current_priority = rule["priority"].as_str().unwrap_or("");
            let priority = {
                let id = id.clone();
                let options = priority_values
                    .iter()
                    .map(|value| {
                        if value.is_empty() {
                            label("settings.rules.priority.keep")
                        } else {
                            tr!(
                                "settings.rules.priority.option",
                                priority = label(&format!("processes.priority.{value}"))
                            )
                        }
                    })
                    .collect();
                self.select(
                    format!("rule-priority-{id}"),
                    tr!("settings.rules.priority.label", name = name),
                    options,
                    priority_values.iter().position(|v| *v == current_priority),
                    false,
                    move |this, index, window, cx| {
                        let value = priority_values[index];
                        let value = if value.is_empty() {
                            Value::Null
                        } else {
                            json!(value)
                        };
                        this.update_rule(&id, json!({"priority":value}), window, cx);
                    },
                    cx,
                )
            };
            let efficiency = {
                let id = id.clone();
                let current = match rule["efficiency"].as_bool() {
                    None => 0,
                    Some(true) => 1,
                    Some(false) => 2,
                };
                self.select(
                    format!("rule-efficiency-{id}"),
                    tr!("settings.rules.efficiency.label", name = name),
                    vec![
                        label("settings.rules.efficiency.keep"),
                        label("settings.rules.efficiency.on"),
                        label("settings.rules.efficiency.off"),
                    ],
                    Some(current),
                    false,
                    move |this, index, window, cx| {
                        let value = match index {
                            1 => json!(true),
                            2 => json!(false),
                            _ => Value::Null,
                        };
                        this.update_rule(&id, json!({"efficiency":value}), window, cx);
                    },
                    cx,
                )
            };
            let toggle = {
                let id = id.clone();
                self.toggle(
                    format!("rule-toggle-{id}"),
                    tr!("settings.rules.toggle", name = name),
                    enabled,
                    false,
                    move |this, on, window, cx| {
                        this.update_rule(&id, json!({"enabled":on}), window, cx)
                    },
                    window,
                    cx,
                )
            };
            let delete = {
                let id = id.clone();
                self.btn_small(
                    format!("rule-delete-{id}"),
                    "",
                    Some("DismissIcon"),
                    Tone::Ghost,
                )
                .accessibility_label(tr!("settings.rules.deleteLabel", name = name))
                .tooltip(|window, cx| widgets::tooltip(label("settings.rules.delete"), window, cx))
                .on_click(cx.listener(move |this, _, window, cx| {
                    let mut rules = values(&this.data["settings"]["rules"]).to_vec();
                    rules.retain(|r| r["id"] != id.as_str());
                    this.request(
                        "set_rules",
                        json!({"rules":rules}),
                        Some("settings"),
                        window,
                        cx,
                    );
                }))
                .into_any_element()
            };
            let focused = self
                .ui
                .focus_rule
                .as_ref()
                .is_some_and(|(rule, _)| *rule == id);
            let card = self.setting(
                &format!("rule-{id}"),
                self.process_icon(process.as_ref(), 20., cx),
                div().font_weight(theme::LABEL_WEIGHT).child(name.clone()),
                Some(summary.into_any_element()),
                None,
                vec![priority, efficiency, toggle, delete],
            );
            cards.push(if focused {
                div()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(p.accent)
                    .child(card)
                    .into_any_element()
            } else {
                card
            });
        }
    }
}
