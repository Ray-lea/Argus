use super::*;
use gpui_kit::base::ElementExt;
use gpui_kit::component::{Disableable, checkbox::Checkbox, dialog::Dialog};
use widgets::{ButtonExt, Field, Tone};

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
        let query = modal.args["query"].as_str().unwrap_or("");
        let stored = modal.args["category"].as_str().unwrap_or("all");
        let category = if query.trim().is_empty() {
            stored
        } else {
            "all"
        };
        let shown = matching(&preferences, category, query);
        let shown_keys = shown.iter().map(|column| column.key).collect::<Vec<_>>();
        let all_selected = shown
            .iter()
            .all(|column| preferences.selected.iter().any(|key| key == column.key));
        let any_selected = shown.iter().any(|column| {
            column.key != "name" && preferences.selected.iter().any(|key| key == column.key)
        });
        let width = f32::from(window.viewport_size().width).min(932.) - 80.;
        let growth = f32::from(theme::text_growth(13., 1.));
        let navigation_width = (150. + growth * 6.).min(220.);
        let two_columns = width - navigation_width - 16. >= 2. * (230. + growth * 7.) + 12.;
        let body_height =
            (f32::from(window.viewport_size().height) - 280. - growth * 5.).clamp(170., 420.);
        let generation = charts::hash_of((id, category, query));
        let choices_scroll = scrollbar::fresh_handle("process-column-choices", generation);
        let category_scroll = scrollbar::fresh_handle("process-column-categories", id);
        let navigation = div()
            .id("column-categories")
            .relative()
            .w(px(navigation_width))
            .h_full()
            .flex_shrink_0()
            .rounded(px(6.))
            .bg(p.layer)
            .p(px(4.))
            .flex()
            .flex_col()
            .gap(px(3.))
            .overflow_y_scroll()
            .track_scroll(&category_scroll)
            .children(
                ["all", "selected"]
                    .into_iter()
                    .chain(CATEGORIES)
                    .map(|key| {
                        let total = if key == "selected" {
                            preferences.selected.len()
                        } else if key == "all" {
                            DEFINITIONS.len()
                        } else {
                            DEFINITIONS.iter().filter(|c| c.category == key).count()
                        };
                        let selected = if matches!(key, "all" | "selected") {
                            preferences.selected.len()
                        } else {
                            DEFINITIONS
                                .iter()
                                .filter(|c| {
                                    c.category == key
                                        && preferences.selected.iter().any(|k| k == c.key)
                                })
                                .count()
                        };
                        let active = category == key;
                        self.btn(
                            format!("column-category-{key}"),
                            label(&format!("processes.columns.category.{key}")),
                            None,
                            Tone::Ghost,
                        )
                        .w_full()
                        .justify_start()
                        .when(active, |button| {
                            button.bg(p.accent.opacity(0.1)).text_color(p.accent)
                        })
                        .child(div().flex_1())
                        .child(
                            div()
                                .text_size(theme::font(11.))
                                .text_color(if active { p.accent } else { p.faint })
                                .child(if key == "selected" {
                                    total.to_string()
                                } else {
                                    format!("{selected}/{total}")
                                }),
                        )
                        .on_prepaint(remember(self, format!("column-category-{key}")))
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                if let Some(modal) = this.modal.as_mut().filter(|m| m.id == id) {
                                    modal.args["category"] = json!(key);
                                    modal.args["query"] = json!("");
                                }
                                if let Some(input) = this.forms.get("columnsSearch").cloned() {
                                    input.update(cx, |state, cx| state.set_value("", window, cx));
                                }
                                window.refresh();
                                cx.notify();
                            },
                        ))
                    }),
            )
            .child(scrollbar::scrollbar(
                "column-category-scrollbar",
                &category_scroll,
                p.dark,
            ));
        let cards = div()
            .grid()
            .grid_cols(if two_columns { 2 } else { 1 })
            .gap(px(10.))
            .children(shown.iter().map(|column| {
                let key = column.key;
                let selected = preferences.selected.iter().any(|k| k == key);
                let title = label(&format!("processes.column.{key}.title"));
                let order = preferences
                    .ordered_selected()
                    .iter()
                    .position(|k| *k == key)
                    .map(|index| index + 1);
                div()
                    .id(format!("column-option-{key}"))
                    .relative()
                    .min_w_0()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(if selected {
                        p.accent.opacity(0.35)
                    } else {
                        p.stroke
                    })
                    .bg(if selected {
                        p.accent.opacity(0.05)
                    } else {
                        p.control_face
                    })
                    .p(px(10.))
                    .hover(move |style| style.bg(p.hover))
                    .on_prepaint(remember(self, format!("column-option-{key}")))
                    .child(
                        Checkbox::new(format!("column-check-{key}"))
                            .w_full()
                            .text_size(theme::font(13.))
                            .label(label(&format!("processes.column.{key}.label")))
                            .checked(selected)
                            .disabled(key == "name")
                            .child(
                                div()
                                    .min_w_0()
                                    .w_full()
                                    .text_size(theme::font(12.))
                                    .line_height(relative(1.4))
                                    .text_color(p.muted)
                                    .line_clamp(2)
                                    .text_ellipsis()
                                    .child(title.clone()),
                            )
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
                    .when(key == "name" || category == "selected", |card| {
                        card.child(
                            div()
                                .absolute()
                                .top(px(7.))
                                .right(px(8.))
                                .text_size(theme::font(10.))
                                .text_color(p.faint)
                                .child(if key == "name" {
                                    label("processes.columns.fixed")
                                } else {
                                    order.unwrap_or_default().to_string()
                                }),
                        )
                    })
                    .tooltip(move |window, cx| widgets::tooltip(title.clone(), window, cx))
            }));
        let mut choices = div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .gap(px(12.));
        let toolbar = div()
            .flex()
            .flex_wrap()
            .items_center()
            .justify_between()
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(div().font_weight(theme::LABEL_WEIGHT).child(label(
                        if !query.trim().is_empty() {
                            "processes.columns.searchResults"
                        } else {
                            match category {
                                "all" => "processes.columns.category.all",
                                "selected" => "processes.columns.category.selected",
                                "cpu" => "processes.columns.category.cpu",
                                "memory" => "processes.columns.category.memory",
                                "io" => "processes.columns.category.io",
                                "network" => "processes.columns.category.network",
                                "gpu" => "processes.columns.category.gpu",
                                _ => "processes.columns.category.general",
                            }
                        },
                    )))
                    .child(
                        div()
                            .text_color(p.faint)
                            .text_size(theme::font(12.))
                            .child(shown.len().to_string()),
                    ),
            )
            .child(
                div().flex().gap(px(4.)).children(
                    [(true, "selectAll"), (false, "clear")]
                        .into_iter()
                        .map(|(enabled, action)| {
                            let keys = shown_keys.clone();
                            self.btn_small(
                                format!("column-{action}"),
                                label(&format!("processes.columns.{action}")),
                                None,
                                Tone::Ghost,
                            )
                            .inert(if enabled { all_selected } else { !any_selected })
                            .on_prepaint(remember(self, format!("column-{action}")))
                            .on_click(cx.listener(
                                move |this, _, window, cx| {
                                    if let Some(modal) = this.modal.as_mut().filter(|m| m.id == id)
                                    {
                                        let mut draft = Preferences::parse(&modal.args["draft"]);
                                        for key in &keys {
                                            draft.set_selected(key, enabled);
                                        }
                                        modal.args["draft"] = json!(draft);
                                        window.refresh();
                                        cx.notify();
                                    }
                                },
                            ))
                        }),
                ),
            );
        choices = choices.child(toolbar).when(
            !boolean(&self.snapshot, "processNetwork")
                && shown.iter().any(|column| column.category == "network"),
            |d| {
                d.child(
                    div()
                        .text_size(theme::font(12.))
                        .text_color(p.muted)
                        .child(label("processes.columns.networkUnavailable")),
                )
            },
        );
        choices = choices.child(
            div()
                .relative()
                .flex_1()
                .min_h_0()
                .child(
                    div()
                        .id("column-choices")
                        .size_full()
                        .overflow_y_scroll()
                        .track_scroll(&choices_scroll)
                        .pr(px(scrollbar::SIZE))
                        .when(shown.is_empty(), |d| {
                            d.child(
                                div()
                                    .h(px(140.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_color(p.muted)
                                    .child(label("processes.columns.noMatch")),
                            )
                        })
                        .when(!shown.is_empty(), |d| d.child(cards)),
                )
                .child(scrollbar::scrollbar(
                    "column-choice-scrollbar",
                    &choices_scroll,
                    p.dark,
                )),
        );
        let search = self.forms.get("columnsSearch").map(|input| {
            self.text_field(
                input,
                Input::new(input).cleanable(true),
                Field::Boxed,
                label("processes.columns.search").into(),
                true,
                window,
                cx,
            )
        });
        dialog
            .child(
                self.dialog_body(id).child(
                    div()
                        .px(px(24.))
                        .pt(px(22.))
                        .pb(px(20.))
                        .flex()
                        .flex_col()
                        .gap(px(16.))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(6.))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .gap(px(12.))
                                        .child(self.dialog_title(modal.title.clone()))
                                        .child(
                                            div()
                                                .flex_shrink_0()
                                                .text_color(p.muted)
                                                .text_size(theme::font(12.))
                                                .child(tr!(
                                                    "processes.columns.selectionSummary",
                                                    count = preferences.selected.len()
                                                )),
                                        ),
                                )
                                .child(
                                    div()
                                        .text_color(p.muted)
                                        .text_size(theme::font(13.))
                                        .child(label("processes.columns.description")),
                                ),
                        )
                        .children(search)
                        .child(
                            div()
                                .flex()
                                .gap(px(16.))
                                .h(px(body_height))
                                .child(navigation)
                                .child(choices),
                        ),
                ),
            )
            .child(
                self.measure(id, 1).child(
                    self.dialog_footer()
                        .justify_between()
                        .flex_wrap()
                        .gap(px(8.))
                        .child(
                            div().flex().items_center().gap(px(10.)).child(
                                self.btn_small(
                                    "column-reset",
                                    label("processes.columns.reset"),
                                    None,
                                    Tone::Ghost,
                                )
                                .on_prepaint(remember(self, "column-reset"))
                                .on_click(cx.listener(
                                    move |this, _, window, cx| {
                                        if let Some(modal) =
                                            this.modal.as_mut().filter(|m| m.id == id)
                                        {
                                            modal.args["draft"] = json!(Preferences::default());
                                            window.refresh();
                                            cx.notify();
                                        }
                                    },
                                )),
                            ),
                        )
                        .child(
                            div()
                                .flex()
                                .gap(px(8.))
                                .child(
                                    self.btn(
                                        "column-apply",
                                        label("processes.columns.apply"),
                                        None,
                                        Tone::Accent,
                                    )
                                    .min_w(px(82.))
                                    .track_focus(
                                        &self.focus_for(&format!("modal-{id}-primary"), cx),
                                    )
                                    .on_prepaint(remember(self, "column-apply"))
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
                                    .min_w(px(82.))
                                    .on_prepaint(remember(self, "column-cancel"))
                                    .on_click(cx.listener(
                                        |this, _, window, cx| this.close_overlay(window, cx),
                                    )),
                                ),
                        ),
                ),
            )
    }
}
