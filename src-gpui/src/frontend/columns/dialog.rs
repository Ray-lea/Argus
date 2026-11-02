use super::*;
use gpui_kit::base::ElementExt;
use gpui_kit::component::{Disableable, checkbox::Checkbox, dialog::Dialog};
use widgets::Tone;

fn remember(
    owner: &Desktop,
    key: impl Into<SharedString>,
) -> impl Fn(Bounds<Pixels>, &mut Window, &mut App) + 'static {
    let anchors = owner.ui.anchors.clone();
    let key = key.into();
    move |bounds, _, _| {
        anchors.borrow_mut().insert(key.clone(), bounds);
    }
}

impl Desktop {
    pub(in crate::frontend) fn process_header_menu(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let preferences = Preferences::load(&self.prefs);
        self.open_popup(
            vec![
                MenuItem::new(
                    label("processes.columns.choose"),
                    Action::Dialog("columns".into(), Value::Null),
                ),
                MenuItem::new(
                    label("processes.columns.autoWidth"),
                    Action::Preference(
                        "processColumnsAutoWidth".into(),
                        json!(!preferences.auto_width),
                    ),
                )
                .checked(preferences.auto_width),
                MenuItem::new(
                    label("processes.columns.reset"),
                    Action::Preference("processColumnsReset".into(), Value::Null),
                ),
            ],
            window,
            cx,
        );
    }

    pub(in crate::frontend) fn set_process_columns(
        &mut self,
        preferences: Preferences,
        cx: &mut Context<Self>,
    ) {
        if self.page == "processes" && !preferences.selected.iter().any(|key| key == &self.sort) {
            self.sort = "name".into();
            self.descending = false;
            self.prefs.set("sort", json!({"key":"name","desc":false}));
            self.rebuild_rows(false, cx);
        }
        self.prefs.set(PREFERENCE, preferences);
        cx.notify();
    }

    pub(in crate::frontend) fn apply_process_columns(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(modal) = self.modal.as_ref().filter(|modal| modal.kind == "columns") else {
            return;
        };
        let preferences = Preferences::parse(&modal.args["draft"]);
        self.set_process_columns(preferences, cx);
        self.close_overlay(window, cx);
    }

    pub(in crate::frontend) fn process_columns_dialog(
        &self,
        dialog: Dialog,
        modal: &dialogs::Modal,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Dialog {
        let id = modal.id;
        let p = self.palette;
        let preferences = Preferences::parse(&modal.args["draft"]);
        let category = modal.args["category"]
            .as_str()
            .filter(|s| CATEGORIES.contains(s))
            .unwrap_or("general");
        let columns = DEFINITIONS
            .iter()
            .filter(|column| column.category == category);
        let narrow = f32::from(window.viewport_size().width) < 680.;
        let categories = div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .w(px(136.))
            .flex_shrink_0()
            .children(CATEGORIES.into_iter().map(|category_key| {
                self.btn(
                    format!("column-category-{category_key}"),
                    label(&format!("processes.columns.category.{category_key}")),
                    None,
                    if category_key == category {
                        Tone::Accent
                    } else {
                        Tone::Plain
                    },
                )
                .w_full()
                .on_prepaint(remember(self, format!("column-category-{category_key}")))
                .on_click(cx.listener(move |this, _, window, cx| {
                    if let Some(modal) = this.modal.as_mut().filter(|m| m.id == id) {
                        modal.args["category"] = json!(category_key);
                        window.refresh();
                        cx.notify();
                    }
                }))
            }));
        let choices = div()
            .flex_1()
            .min_w_0()
            .self_start()
            .grid()
            .grid_cols(if narrow { 1 } else { 2 })
            .gap(px(12.))
            .children(columns.map(|column| {
                let key = column.key;
                let checked = preferences.selected.iter().any(|saved| saved == key);
                let anchors = self.ui.anchors.clone();
                div()
                    .id(format!("column-option-{key}"))
                    .on_prepaint(move |bounds, _, _| {
                        anchors
                            .borrow_mut()
                            .insert(format!("column-option-{key}").into(), bounds);
                    })
                    .min_w_0()
                    .min_h(px(26.))
                    .child(
                        Checkbox::new(format!("column-check-{key}"))
                            .w_full()
                            .text_size(theme::font(13.))
                            .label(label(&format!("processes.column.{key}.label")))
                            .checked(checked)
                            .disabled(key == "name")
                            .on_click(cx.listener(move |this, enabled, window, cx| {
                                if let Some(modal) = this.modal.as_mut().filter(|m| m.id == id) {
                                    let mut draft = Preferences::parse(&modal.args["draft"]);
                                    draft.set_selected(key, *enabled);
                                    modal.args["draft"] = json!(draft);
                                    window.refresh();
                                    cx.notify();
                                }
                            })),
                    )
                    .tooltip({
                        let description = label(&format!("processes.column.{key}.title"));
                        move |window, cx| widgets::tooltip(description.clone(), window, cx)
                    })
            }));
        dialog
            .child(
                self.dialog_body(id).child(
                    div()
                        .p(px(24.))
                        .flex()
                        .flex_col()
                        .gap(px(18.))
                        .child(self.dialog_title(modal.title.clone()))
                        .child(
                            div()
                                .text_color(p.muted)
                                .child(label("processes.columns.description")),
                        )
                        .child(
                            div()
                                .flex()
                                .gap(px(20.))
                                .min_h(px(240.))
                                .child(categories)
                                .child(choices),
                        )
                        .child(
                            div()
                                .text_color(p.faint)
                                .text_size(theme::font(12.))
                                .child(label(
                                    if category == "network"
                                        && !boolean(&self.snapshot, "processNetwork")
                                    {
                                        "processes.columns.networkUnavailable"
                                    } else {
                                        "processes.columns.nameRequired"
                                    },
                                )),
                        ),
                ),
            )
            .child(
                self.measure(id, 1).child(
                    self.dialog_footer()
                        .justify_between()
                        .flex_wrap()
                        .child(
                            self.btn(
                                "column-reset",
                                label("processes.columns.reset"),
                                None,
                                Tone::Plain,
                            )
                            .on_prepaint(remember(self, "column-reset"))
                            .on_click(cx.listener(
                                move |this, _, window, cx| {
                                    if let Some(modal) = this.modal.as_mut().filter(|m| m.id == id)
                                    {
                                        modal.args["draft"] = json!(Preferences::default());
                                        window.refresh();
                                        cx.notify();
                                    }
                                },
                            )),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .child(
                                    div()
                                        .mr(px(8.))
                                        .text_color(p.muted)
                                        .text_size(theme::font(12.))
                                        .child(tr!(
                                            "processes.columns.selected",
                                            count = preferences.selected.len()
                                        )),
                                )
                                .child(
                                    self.btn(
                                        "column-apply",
                                        label("processes.columns.apply"),
                                        None,
                                        Tone::Accent,
                                    )
                                    .on_prepaint(remember(self, "column-apply"))
                                    .min_w(px(82.))
                                    .track_focus(
                                        &self.focus_for(&format!("modal-{id}-primary"), cx),
                                    )
                                    .on_click(cx.listener(
                                        |this, _, window, cx| {
                                            this.apply_process_columns(window, cx)
                                        },
                                    )),
                                )
                                .child(
                                    self.btn(
                                        "column-cancel",
                                        label("common.cancel"),
                                        None,
                                        Tone::Plain,
                                    )
                                    .on_prepaint(remember(self, "column-cancel"))
                                    .min_w(px(82.))
                                    .on_click(cx.listener(
                                        |this, _, window, cx| this.close_overlay(window, cx),
                                    )),
                                ),
                        ),
                ),
            )
    }
}
