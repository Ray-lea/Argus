//! Display-only process filtering. Collection and system totals remain complete.
use super::dialogs::Modal;
use super::*;
use gpui_kit::component::dialog::Dialog;
use std::collections::BTreeSet;
use widgets::{Field, Tone};

pub(super) const RULES: &str = "processVisibility";
pub(super) fn preference(page: &str) -> &'static str {
    if page == "network" {
        "hideSystem.network"
    } else {
        "hideSystem.processes"
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub(super) struct Rules {
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

/// Exact executable names, never paths or wildcard patterns.
fn normalize(value: &str) -> Result<BTreeSet<String>, ()> {
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

impl Desktop {
    pub(in crate::frontend) fn visibility_rules(&self, page: &str) -> Option<Rules> {
        (matches!(page, "processes" | "network") && self.prefs.get(preference(page), false))
            .then(|| Rules::load(&self.prefs))
    }

    pub(in crate::frontend) fn visibility_controls(&self, cx: &Context<Self>) -> AnyElement {
        let active = self.prefs.get(preference(&self.page), false);
        let key = preference(&self.page);
        let p = self.palette;
        let title = label("processes.visibility.toggle");
        let hint = label(if active {
            "processes.visibility.show"
        } else if self.page == "network" {
            "processes.visibility.networkHint"
        } else {
            "processes.visibility.processHint"
        });
        let anchors = self.ui.anchors.clone();
        div()
            .flex()
            .items_center()
            .flex_shrink_0()
            .child(
                div()
                    .relative()
                    .child(
                        self.tool_button(
                            "hide-system",
                            "",
                            Some("EyeOffIcon"),
                            Action::Preference(key.into(), json!(!active)),
                            active,
                            cx,
                        )
                        .selected(active)
                        .accessibility_label(if active {
                            label("processes.visibility.show")
                        } else {
                            title
                        })
                        .when(active, |b| {
                            b.bg(p.accent.opacity(0.14))
                                .border_color(p.accent.opacity(0.5))
                        })
                        .tooltip(move |window, cx| {
                            widgets::wrapping_tooltip(hint.clone().into(), px(320.), window, cx)
                        }),
                    )
                    .child(
                        canvas(
                            move |bounds, _, _| {
                                anchors.borrow_mut().insert("hide-system".into(), bounds);
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .inset_0(),
                    ),
            )
            .into_any_element()
    }

    pub(in crate::frontend) fn visibility_editor_button(&self, cx: &Context<Self>) -> AnyElement {
        let anchors = self.ui.anchors.clone();
        div()
            .relative()
            .child(
                self.btn(
                    "visibility-edit",
                    label("processes.visibility.edit"),
                    None,
                    Tone::Plain,
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.open_dialog("visibility", json!({}), window, cx)
                })),
            )
            .child(
                canvas(
                    move |bounds, _, _| {
                        anchors
                            .borrow_mut()
                            .insert("visibility-edit".into(), bounds);
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0(),
            )
            .into_any_element()
    }

    pub(in crate::frontend) fn visibility_forms(
        &mut self,
        rules: &Rules,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for (key, names) in [
            ("visibilityKeep", &rules.keep),
            ("visibilityHide", &rules.hide),
        ] {
            let value = names.iter().cloned().collect::<Vec<_>>().join(", ");
            if let Some(input) = self.forms.get(key).cloned() {
                input.update(cx, |state, cx| state.set_value(value, window, cx));
            } else {
                self.form(key, &value, window, cx);
            }
        }
        if let Some(modal) = &mut self.modal {
            modal.error = None;
        }
    }

    pub(in crate::frontend) fn save_visibility(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let read = |key| {
            self.forms
                .get(key)
                .map(|s| s.read(cx).value().to_string())
                .unwrap_or_default()
        };
        let (Ok(keep), Ok(hide)) = (
            normalize(&read("visibilityKeep")),
            normalize(&read("visibilityHide")),
        ) else {
            if let Some(modal) = &mut self.modal {
                modal.error = Some(label("processes.visibility.invalid"));
            }
            cx.notify();
            return;
        };
        self.prefs.set(RULES, Rules { keep, hide });
        self.rebuild_rows(false, cx);
        self.close_overlay(window, cx);
    }

    pub(in crate::frontend) fn visibility_dialog(
        &self,
        dialog: Dialog,
        modal: &Modal,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Dialog {
        let p = self.palette;
        let mut body = div()
            .p(px(24.))
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(self.dialog_title(modal.title.clone()))
            .child(
                div()
                    .text_color(p.muted)
                    .child(label("processes.visibility.description")),
            );
        for (key, label_key, hint_key) in [
            (
                "visibilityKeep",
                "processes.visibility.keep",
                "processes.visibility.keepHint",
            ),
            (
                "visibilityHide",
                "processes.visibility.hide",
                "processes.visibility.hideHint",
            ),
        ] {
            if let Some(input) = self.forms.get(key) {
                body = body.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .child(label(label_key))
                        .child(self.text_field(
                            input,
                            widgets::input(input),
                            Field::Boxed,
                            label(label_key).into(),
                            true,
                            window,
                            cx,
                        ))
                        .child(
                            div()
                                .text_color(p.muted)
                                .text_size(theme::font(12.))
                                .child(label(hint_key)),
                        ),
                );
            }
        }
        body = body
            .child(
                div()
                    .text_size(theme::font(12.))
                    .text_color(p.muted)
                    .child(label("processes.visibility.namesHint")),
            )
            .when_some(modal.error.clone(), |d, error| {
                d.child(div().text_color(p.danger).child(error))
            });
        dialog.child(self.dialog_body(modal.id).child(body)).footer(
            self.measure(modal.id, 1).child(
                self.dialog_footer()
                    .child(
                        self.btn(
                            "visibility-save",
                            label("common.marks.save"),
                            None,
                            Tone::Accent,
                        )
                        .track_focus(&self.primary_focus(modal.id, cx))
                        .on_click(
                            cx.listener(|this, _, window, cx| this.save_visibility(window, cx)),
                        ),
                    )
                    .child(
                        self.btn(
                            "visibility-reset",
                            label("processes.visibility.reset"),
                            None,
                            Tone::Plain,
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.visibility_forms(&Rules::default(), window, cx);
                            cx.notify();
                        })),
                    )
                    .child(
                        self.btn(
                            "visibility-cancel",
                            label("common.cancel"),
                            None,
                            Tone::Plain,
                        )
                        .on_click(
                            cx.listener(|this, _, window, cx| this.close_overlay(window, cx)),
                        ),
                    ),
            ),
        )
    }
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
