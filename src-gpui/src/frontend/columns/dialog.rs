//! The process header's menu and the column dialog: the columns by category
//! to check on the left, the shown ones in order on the right to drag.
use super::*;
use gpui_kit::base::ElementExt;
use gpui_kit::component::dialog::Dialog;
use widgets::{ButtonExt, Field, Tone};

/// The order list beside the choices: about a third of the body, within these bounds.
const ORDER_MIN: f32 = 220.;
const ORDER_MAX: f32 = 280.;
/// A choice to check.
const ROW: f32 = 34.;
/// A place in the order list, and the distance from one place to the next.
const PLACE: f32 = 32.;
const PITCH: f32 = PLACE + 2.;
/// Rows joining the order list fade in over this long.
const ARRIVE: std::time::Duration = std::time::Duration::from_millis(160);
/// Another category's choices fade in over this long.
const SWITCH: std::time::Duration = std::time::Duration::from_millis(140);

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

fn category_icon(category: &str) -> &'static str {
    match category {
        "cpu" => "CpuIcon",
        "memory" => "MemoryIcon",
        "io" => "DiskIcon",
        "network" => "NetworkIcon",
        "gpu" => "GpuIcon",
        _ => "ProcessesIcon",
    }
}

fn column_label(key: &str) -> String {
    label(&format!("processes.column.{key}.label"))
}

/// Where a dragged place moves once its middle passes a neighbour's: the
/// place it goes before (`count` for past the last), never before the fixed
/// first one; none while it stays between its neighbours.
fn order_step(index: usize, count: usize, middle: f32) -> Option<usize> {
    let middle_of = |at: usize| at as f32 * PITCH + PLACE / 2.;
    if index + 1 < count && middle > middle_of(index + 1) {
        Some(index + 2)
    } else if index > 1 && middle < middle_of(index - 1) {
        Some(index - 1)
    } else {
        None
    }
}

/// A shown column being dragged to another place.
struct ColumnDrag;

/// Nothing follows the pointer but the place itself.
struct Lifted;

impl Render for Lifted {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

impl Desktop {
    /// The process header's menu: the column under the pointer sorted or
    /// hidden; every column by category, checked in place; then the dialog,
    /// automatic widths and the defaults. `at` places it other than at the pointer.
    pub(in crate::frontend) fn process_header_menu(
        &mut self,
        column: Option<String>,
        at: Option<Point<Pixels>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let preferences = Preferences::load(&self.prefs);
        let shown = |key: &str| preferences.selected.iter().any(|saved| saved == key);
        let mut items = Vec::new();
        if self.view == "tree" {
            let searching = !self.query.trim().is_empty();
            for (expand, title, glyph) in [
                (true, "processes.tree.expandAll", "ExpandAllIcon"),
                (false, "processes.tree.collapseAll", "CollapseAllIcon"),
            ] {
                let disabled = searching
                    || if expand {
                        !self.rows.iter().any(|row| row.expandable && !row.expanded)
                    } else {
                        self.tree_expanded.is_empty()
                    };
                items.push(
                    MenuItem::new(label(title), Action::TreeExpandAll(expand))
                        .icon(glyph)
                        .disabled(disabled),
                );
            }
            items.push(MenuItem::separator());
        }
        if let Some(key) = column
            .as_deref()
            .and_then(definition)
            .map(|column| column.key)
        {
            let sortable = tables::sortable("processes", key);
            for (descending, text, glyph) in [
                (
                    false,
                    "processes.columns.sortAscending",
                    "SortAscendingIcon",
                ),
                (
                    true,
                    "processes.columns.sortDescending",
                    "SortDescendingIcon",
                ),
            ] {
                items.push(
                    MenuItem::new(
                        label(text),
                        Action::Preference(
                            "processSort".into(),
                            json!({"key": key, "desc": descending}),
                        ),
                    )
                    .icon(glyph)
                    .disabled(!sortable),
                );
            }
            items.push(
                MenuItem::new(
                    tr!("processes.columns.hide", name = column_label(key)),
                    Action::Preference("toggleProcessColumn".into(), json!(key)),
                )
                .icon("EyeOffIcon")
                .disabled(key == "name"),
            );
            items.push(MenuItem::separator());
        }
        for category in CATEGORIES {
            let children = DEFINITIONS
                .iter()
                .filter(|column| column.category == category)
                .map(|column| {
                    MenuItem::new(
                        column_label(column.key),
                        Action::Preference("toggleProcessColumn".into(), json!(column.key)),
                    )
                    .checked(shown(column.key))
                    .disabled(column.key == "name")
                    .stay()
                })
                .collect::<Vec<_>>();
            let checked = children.iter().filter(|item| item.checked).count();
            items.push(
                MenuItem::new(
                    label(&format!("processes.columns.category.{category}")),
                    Action::CloseOverlay,
                )
                .icon(category_icon(category))
                .detail(format!("{checked}/{}", children.len()))
                .submenu(children),
            );
        }
        items.push(MenuItem::separator());
        items.push(
            MenuItem::new(
                label("processes.columns.chooseMore"),
                Action::Dialog("columns".into(), Value::Null),
            )
            .icon("ColumnsIcon"),
        );
        items.push(
            MenuItem::new(
                label("processes.columns.autoWidth"),
                Action::Preference(
                    "processColumnsAutoWidth".into(),
                    json!(!preferences.auto_width),
                ),
            )
            .checked(preferences.auto_width),
        );
        items.push(
            MenuItem::new(
                label("processes.columns.reset"),
                Action::Preference("processColumnsReset".into(), Value::Null),
            )
            .icon("ResetIcon"),
        );
        match at {
            Some(at) => self.open_popup_at(items, at, window, cx),
            None => self.open_popup(items, window, cx),
        }
    }

    /// Sorts the process list by `key` in the given direction.
    pub(in crate::frontend) fn sort_processes(
        &mut self,
        key: &str,
        descending: bool,
        cx: &mut Context<Self>,
    ) {
        if !tables::sortable("processes", key) {
            return;
        }
        self.sort = key.into();
        self.descending = descending;
        self.sorts
            .insert("processes".into(), (self.sort.clone(), self.descending));
        if let Some((key, value)) = tables::sort_preference("processes", key, descending) {
            self.prefs.set(&key, value);
        }
        self.rebuild_rows(false, cx);
        cx.notify();
    }

    /// Shows a hidden column or hides a shown one, at once.
    pub(in crate::frontend) fn toggle_process_column(&mut self, key: &str, cx: &mut Context<Self>) {
        let mut preferences = Preferences::load(&self.prefs);
        let shown = preferences.selected.iter().any(|saved| saved == key);
        preferences.set_selected(key, !shown);
        self.set_process_columns(preferences, cx);
    }

    /// Shows another category of the dialog's choices.
    pub(in crate::frontend) fn column_category(
        &mut self,
        category: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(modal) = self.modal.as_mut().filter(|modal| modal.kind == "columns") {
            modal.args["category"] = json!(category);
            modal.args["query"] = json!("");
        }
        if let Some(input) = self.forms.get("columnsSearch").cloned() {
            input.update(cx, |state, cx| state.set_value("", window, cx));
        }
        window.refresh();
        cx.notify();
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

    /// Changes the dialog's draft; nothing is saved before "OK".
    fn edit_columns(
        &mut self,
        id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
        edit: impl FnOnce(&mut Preferences),
    ) {
        if let Some(modal) = self.modal.as_mut().filter(|modal| modal.id == id) {
            let mut draft = Preferences::parse(&modal.args["draft"]);
            edit(&mut draft);
            modal.args["draft"] = json!(draft);
            window.refresh();
            cx.notify();
        }
    }

    /// Moves a shown column before the one now at `place` (past the last for none).
    fn move_column(
        &mut self,
        id: u64,
        key: &'static str,
        place: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.edit_columns(id, window, cx, |draft| {
            let order = draft
                .ordered_selected()
                .into_iter()
                .map(String::from)
                .collect::<Vec<_>>();
            draft.move_before(key, order.get(place).map(String::as_str));
        });
    }

    /// Follows a drag along the order list: once the dragged place's middle
    /// passes a neighbour's, the two trade places at once (the draft changes)
    /// and the neighbour glides to its new place.
    fn order_drag_move(
        &mut self,
        id: u64,
        event: &DragMoveEvent<ColumnDrag>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(mut drag) = self.ui.order_drag.borrow().clone() else {
            return;
        };
        drag.pointer = f32::from(event.event.position.y);
        *self.ui.order_drag.borrow_mut() = Some(drag.clone());
        let index_of = |this: &Self| {
            let modal = this.modal.as_ref().filter(|modal| modal.id == id)?;
            let draft = Preferences::parse(&modal.args["draft"]);
            let order = draft.ordered_selected();
            let index = order.iter().position(|key| *key == drag.key)?;
            Some((index, order.len()))
        };
        while let Some((index, count)) = index_of(self)
            && let Some(place) = order_step(index, count, drag.at() + PLACE / 2.)
            && let Some(key) = definition(&drag.key).map(|column| column.key)
        {
            self.move_column(id, key, place, window, cx);
            // A move the draft refused would otherwise be tried forever.
            if index_of(self).map(|(after, _)| after) == Some(index) {
                break;
            }
        }
        cx.notify();
    }

    /// Ends a drag in the order list: the place glides from under the
    /// pointer into its slot.
    fn settle_order_drag(&self, order: &[&str], animate: bool) {
        let Some(drag) = self.ui.order_drag.borrow_mut().take() else {
            return;
        };
        let Some(index) = order.iter().position(|key| *key == drag.key) else {
            return;
        };
        let place = index as f32 * PITCH;
        self.ui.order_flips.quiet(&drag.key, place);
        if animate {
            self.ui
                .order_flips
                .moved(&drag.key, place - drag.at(), Instant::now());
        }
    }

    /// A check box as the system draws one: an accent square with a tick
    /// when checked, a stroked one when not; quieter for a fixed choice.
    fn check_box(&self, checked: bool, fixed: bool) -> Div {
        let p = self.palette;
        let base = div()
            .size(px(18.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(4.));
        if checked {
            base.bg(if fixed {
                p.accent.opacity(0.45)
            } else {
                p.accent
            })
            .child(icon("CheckIcon", p.on_accent).size(px(12.)))
        } else {
            base.border_1()
                .border_color(p.stroke_strong)
                .bg(p.control_face)
        }
    }

    /// One column to check: its name and, quieter, what it shows.
    fn column_choice(
        &self,
        id: u64,
        column: &'static Definition,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.palette;
        let key = column.key;
        let fixed = key == "name";
        let name = column_label(key);
        let title = label(&format!("processes.column.{key}.title"));
        Button::new(SharedString::from(format!("column-option-{key}")))
            .h(px(ROW))
            .w_full()
            .flex()
            .items_center()
            .justify_start()
            .gap(px(10.))
            .px(px(8.))
            .rounded(px(6.))
            .text_size(theme::font(13.))
            .text_color(p.ink)
            .role(Role::CheckBox)
            .aria_toggled(if selected {
                Toggled::True
            } else {
                Toggled::False
            })
            .accessibility_label(name.clone())
            .when(!fixed, |row| {
                row.hover(move |style| style.bg(p.hover))
                    .active(move |style| style.bg(p.press))
            })
            .disabled(fixed)
            .child(self.check_box(selected, fixed))
            .child(
                div()
                    .flex_shrink_0()
                    .max_w(relative(0.45))
                    .truncate()
                    .child(name),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(theme::font(12.))
                    .text_color(p.muted)
                    .child(title.clone()),
            )
            .when(fixed, |row| {
                row.child(
                    div()
                        .flex_shrink_0()
                        .px(px(6.))
                        .rounded(px(4.))
                        .bg(p.ink.opacity(0.06))
                        .text_size(theme::font(11.))
                        .line_height(px(18.))
                        .text_color(p.muted)
                        .child(label("processes.columns.fixed")),
                )
            })
            .tooltip(move |window, cx| widgets::tooltip(title.clone(), window, cx))
            .on_prepaint(remember(self, format!("column-option-{key}")))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.edit_columns(id, window, cx, |draft| draft.set_selected(key, !selected));
            }))
            .into_any_element()
    }

    /// A category's heading, with its count and a switch for all of its
    /// shown choices, then the choices.
    fn column_section(
        &self,
        id: u64,
        category: &'static str,
        columns: &[&'static Definition],
        preferences: &Preferences,
        network_note: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.palette;
        let selected = |key: &str| preferences.selected.iter().any(|saved| saved == key);
        let keys = columns
            .iter()
            .map(|column| column.key)
            .filter(|key| *key != "name")
            .collect::<Vec<_>>();
        let all = keys.iter().all(|key| selected(key));
        let in_category = DEFINITIONS
            .iter()
            .filter(|column| column.category == category);
        let count = format!(
            "{}/{}",
            in_category
                .clone()
                .filter(|column| selected(column.key))
                .count(),
            in_category.count()
        );
        let head = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .h(px(30.))
            .px(px(8.))
            .child(icon(category_icon(category), p.muted).size(px(14.)))
            .child(
                div()
                    .text_size(theme::font(12.))
                    .font_weight(theme::LABEL_WEIGHT)
                    .text_color(p.muted)
                    .child(label(&format!("processes.columns.category.{category}"))),
            )
            .child(
                div()
                    .text_size(theme::font(11.))
                    .text_color(p.faint)
                    .child(count),
            )
            .child(div().flex_1())
            .when(!keys.is_empty(), |head| {
                head.child(
                    self.btn_small(
                        format!("column-group-{category}"),
                        label(if all {
                            "processes.columns.clear"
                        } else {
                            "processes.columns.selectAll"
                        }),
                        None,
                        Tone::Ghost,
                    )
                    .text_color(p.accent)
                    .on_prepaint(remember(self, format!("column-group-{category}")))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        let keys = keys.clone();
                        this.edit_columns(id, window, cx, |draft| {
                            for key in &keys {
                                draft.set_selected(key, !all);
                            }
                        });
                    })),
                )
            });
        div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .child(head)
            .when(network_note && category == "network", |section| {
                section.child(
                    div()
                        .px(px(8.))
                        .pb(px(4.))
                        .text_size(theme::font(12.))
                        .text_color(p.muted)
                        .child(label("processes.columns.networkUnavailable")),
                )
            })
            .children(
                columns
                    .iter()
                    .map(|column| self.column_choice(id, column, selected(column.key), cx)),
            )
            .into_any_element()
    }

    /// A small icon button of the order list.
    fn order_button(
        &self,
        id: String,
        glyph: &'static str,
        tip: String,
        enabled: bool,
        cx: &mut Context<Self>,
        on: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> AnyElement {
        self.btn_small(id, "", Some(glyph), Tone::Ghost)
            .inert_dimmed(!enabled)
            .accessibility_label(tip.clone())
            .tooltip(move |window, cx| widgets::tooltip(tip.clone(), window, cx))
            .on_click(cx.listener(move |this, _, window, cx| on(this, window, cx)))
            .into_any_element()
    }

    /// What a place in the order list shows: the grip (a lock for the fixed
    /// name), the column's name and its category.
    fn order_face(
        &self,
        column: &'static Definition,
        group: Option<SharedString>,
    ) -> Vec<AnyElement> {
        let p = self.palette;
        let fixed = column.key == "name";
        vec![
            icon(if fixed { "LockIcon" } else { "GripIcon" }, p.faint)
                .size(px(14.))
                .into_any_element(),
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .child(column_label(column.key))
                .into_any_element(),
            div()
                .flex_shrink_0()
                .text_size(theme::font(11.))
                .text_color(p.faint)
                .when_some(group, |tag, group| {
                    tag.group_hover(group, |style| style.invisible())
                })
                .child(label(&if fixed {
                    "processes.columns.fixed".to_owned()
                } else {
                    format!("processes.columns.category.{}", column.category)
                }))
                .into_any_element(),
        ]
    }

    /// A shown column in its place: dragged elsewhere, or moved and removed
    /// with the buttons that replace its category under the pointer. A place
    /// laid out somewhere new glides there; the dragged one leaves a tinted
    /// slot while it is drawn under the pointer (`column_order`).
    #[allow(clippy::too_many_arguments)]
    fn order_row(
        &self,
        id: u64,
        index: usize,
        count: usize,
        column: &'static Definition,
        drag: Option<&flip::Drag>,
        now: Instant,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.palette;
        let key = column.key;
        let fixed = key == "name";
        let place = index as f32 * PITCH;
        let lifted = drag.is_some_and(|drag| drag.key == key);
        let flips = &self.ui.order_flips;
        if lifted || cx.reduce_motion() {
            flips.quiet(key, place);
        } else {
            flips.place(key, place, now);
        }
        let dy = flips.offset(key, now);
        let group = SharedString::from(format!("column-order-row-{key}"));
        let row = div()
            .id(SharedString::from(format!("column-order-{key}")))
            .group(group.clone())
            .relative()
            .flex_shrink_0()
            .h(px(PLACE))
            .px(px(8.))
            .flex()
            .items_center()
            .gap(px(8.))
            .rounded(px(6.))
            .text_size(theme::font(13.))
            .text_color(p.ink)
            .on_prepaint(remember(self, format!("column-order-{index}")))
            // The slot the dragged place will drop into, empty and tinted.
            .when(lifted, |row| row.bg(p.accent.opacity(0.08)))
            .when(!lifted, |row| {
                row.children(self.order_face(column, (!fixed).then(|| group.clone())))
            });
        let row = if fixed {
            row
        } else {
            let up = (index > 1).then(|| index - 1);
            let down = (index + 1 < count).then(|| index + 2);
            let desktop = cx.entity().downgrade();
            row.cursor_grab()
                .when(drag.is_none(), |row| {
                    row.hover(move |style| style.bg(p.hover)).child(
                        div()
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .right(px(4.))
                            .flex()
                            .items_center()
                            .gap(px(2.))
                            .invisible()
                            .group_hover(group, |style| style.visible())
                            .child(self.order_button(
                                format!("column-up-{key}"),
                                "ChevronUp",
                                label("processes.columns.moveUp"),
                                up.is_some(),
                                cx,
                                move |this, window, cx| {
                                    if let Some(place) = up {
                                        this.move_column(id, key, place, window, cx);
                                    }
                                },
                            ))
                            .child(self.order_button(
                                format!("column-down-{key}"),
                                "ChevronDown",
                                label("processes.columns.moveDown"),
                                down.is_some(),
                                cx,
                                move |this, window, cx| {
                                    if let Some(place) = down {
                                        this.move_column(id, key, place, window, cx);
                                    }
                                },
                            ))
                            .child(self.order_button(
                                format!("column-remove-{key}"),
                                "DismissIcon",
                                label("processes.columns.remove"),
                                true,
                                cx,
                                move |this, window, cx| {
                                    this.edit_columns(id, window, cx, |draft| {
                                        draft.set_selected(key, false)
                                    });
                                },
                            )),
                    )
                })
                .on_drag(ColumnDrag, move |_, _, window, cx| {
                    let pointer = f32::from(window.mouse_position().y);
                    let _ = desktop.update(cx, |this, cx| {
                        *this.ui.order_drag.borrow_mut() = Some(flip::Drag {
                            key: key.to_owned(),
                            grab: pointer,
                            pointer,
                            start: place,
                        });
                        cx.notify();
                    });
                    cx.new(|_| Lifted)
                })
        };
        let row = if cx.reduce_motion() {
            row.into_any_element()
        } else {
            row.with_animation(
                SharedString::from(format!("column-order-arrive-{key}")),
                Animation::new(ARRIVE).with_easing(theme::ease_out),
                |row, t| row.opacity(t),
            )
            .into_any_element()
        };
        super::super::shift::shifted(row, point(px(0.), px(dy)), 1.).into_any_element()
    }

    /// The shown columns in their order, the fixed name first. A dragged
    /// place is drawn raised under the pointer, over the others.
    fn column_order(
        &self,
        id: u64,
        preferences: &Preferences,
        width: f32,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.palette;
        let order = preferences.ordered_selected();
        let now = Instant::now();
        // A drag let go away from the list (no drop there) settles too.
        if !cx.has_active_drag() {
            self.settle_order_drag(&order, !cx.reduce_motion());
        }
        let drag = self.ui.order_drag.borrow().clone();
        let columns = order
            .iter()
            .filter_map(|key| definition(key))
            .collect::<Vec<_>>();
        let count = columns.len();
        let rows = columns
            .iter()
            .enumerate()
            .map(|(index, column)| self.order_row(id, index, count, column, drag.as_ref(), now, cx))
            .collect::<Vec<_>>();
        if drag.is_some() || self.ui.order_flips.active(now) {
            window.request_animation_frame();
        }
        let raised = drag.as_ref().and_then(|drag| {
            let column = definition(&drag.key)?;
            let top = drag.at().clamp(0., (count.max(1) - 1) as f32 * PITCH);
            Some(
                div()
                    .absolute()
                    .left(px(6.))
                    .right(px(6.))
                    .top(px(top))
                    .h(px(PLACE))
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(p.accent.opacity(0.6))
                    .bg(widgets::solid(p.flyout))
                    .text_size(theme::font(13.))
                    .text_color(p.ink)
                    .cursor_grabbing()
                    .children(self.order_face(column, None)),
            )
        });
        let scroll = scrollbar::fresh_handle("process-column-order", id);
        div()
            .w(px(width))
            .h_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .rounded(px(8.))
            .border_1()
            .border_color(p.stroke)
            .bg(p.layer)
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .justify_between()
                    .gap(px(8.))
                    .px(px(14.))
                    .pt(px(12.))
                    .pb(px(8.))
                    .child(
                        div()
                            .text_size(theme::font(13.))
                            .font_weight(theme::LABEL_WEIGHT)
                            .child(label("processes.columns.order")),
                    )
                    .child(
                        div()
                            .text_size(theme::font(12.))
                            .text_color(p.faint)
                            .child(tr!("processes.columns.selectionSummary", count = count)),
                    ),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(
                        div()
                            .id("column-order")
                            .relative()
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&scroll)
                            .px(px(6.))
                            .pb(px(6.))
                            .flex()
                            .flex_col()
                            .gap(px(PITCH - PLACE))
                            .children(rows)
                            .children(raised)
                            .on_drag_move(cx.listener(
                                move |this, event: &DragMoveEvent<ColumnDrag>, window, cx| {
                                    this.order_drag_move(id, event, window, cx)
                                },
                            ))
                            .on_drop(cx.listener(move |this, _: &ColumnDrag, _, cx| {
                                let order = this
                                    .modal
                                    .as_ref()
                                    .map(|modal| Preferences::parse(&modal.args["draft"]))
                                    .unwrap_or_default();
                                this.settle_order_drag(
                                    &order.ordered_selected(),
                                    !cx.reduce_motion(),
                                );
                                cx.notify();
                            })),
                    )
                    .child(scrollbar::scrollbar(
                        "column-order-scrollbar",
                        &scroll,
                        p.dark,
                    )),
            )
            .into_any_element()
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
        let searching = !query.trim().is_empty();
        let category = if searching {
            "all"
        } else {
            modal.args["category"].as_str().unwrap_or("all")
        };
        let shown = matching(category, query);
        let growth = f32::from(theme::text_growth(13., 1.));
        let width = f32::from(window.viewport_size().width).min(932.) - 80.;
        let order_width = (width * 0.3).clamp(ORDER_MIN, ORDER_MAX) + growth * 4.;
        let body_height =
            (f32::from(window.viewport_size().height) - 280. - growth * 5.).clamp(200., 440.);
        let categories = self.segments(
            "column-categories",
            ["all"]
                .into_iter()
                .chain(CATEGORIES)
                .map(|key| {
                    (
                        label(&format!("processes.columns.category.{key}")),
                        None,
                        Action::Preference("columnsCategory".into(), json!(key)),
                        category == key,
                    )
                })
                .collect(),
            cx,
        );
        let network_note = !boolean(&self.snapshot, "processNetwork");
        let sections = CATEGORIES
            .into_iter()
            .filter_map(|group| {
                let columns = shown
                    .iter()
                    .copied()
                    .filter(|column| column.category == group)
                    .collect::<Vec<_>>();
                (!columns.is_empty()).then(|| {
                    self.column_section(id, group, &columns, &preferences, network_note, cx)
                })
            })
            .collect::<Vec<_>>();
        let generation = charts::hash_of((id, category, query));
        let choices_scroll = scrollbar::fresh_handle("process-column-choices", generation);
        let list = div()
            .id("column-choices")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&choices_scroll)
            .pr(px(scrollbar::SIZE))
            .when(sections.is_empty(), |list| {
                list.child(
                    div()
                        .h(px(140.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(p.muted)
                        .child(label("processes.columns.noMatch")),
                )
            })
            .child(div().flex().flex_col().gap(px(10.)).children(sections));
        // Another category's choices fade in rather than replace the last at once.
        let list = if cx.reduce_motion() {
            list.into_any_element()
        } else {
            list.with_animation(
                SharedString::from(format!("column-choices-{category}")),
                Animation::new(SWITCH).with_easing(theme::ease_out),
                |list, t| list.opacity(0.35 + 0.65 * t),
            )
            .into_any_element()
        };
        let choices =
            div()
                .flex_1()
                .min_w_0()
                .h_full()
                .flex()
                .flex_col()
                .gap(px(10.))
                .child(div().flex().child(categories))
                .child(div().relative().flex_1().min_h_0().child(list).child(
                    scrollbar::scrollbar("column-choice-scrollbar", &choices_scroll, p.dark),
                ));
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
                                .child(self.dialog_title(modal.title.clone()))
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
                                .child(choices)
                                .child(self.column_order(
                                    id,
                                    &preferences,
                                    order_width,
                                    window,
                                    cx,
                                )),
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
                            self.btn_small(
                                "column-reset",
                                label("processes.columns.reset"),
                                Some("ResetIcon"),
                                Tone::Ghost,
                            )
                            .on_prepaint(remember(self, "column-reset"))
                            .on_click(cx.listener(
                                move |this, _, window, cx| {
                                    this.edit_columns(id, window, cx, |draft| {
                                        *draft = Preferences::default();
                                    });
                                },
                            )),
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

#[cfg(test)]
mod tests {
    use super::{PITCH, PLACE, order_step};

    #[test]
    fn a_dragged_place_trades_with_a_neighbour_once_past_its_middle() {
        let middle = |index: usize| index as f32 * PITCH + PLACE / 2.;
        // Five places; the third (index 2) dragged.
        assert_eq!(order_step(2, 5, middle(2)), None);
        assert_eq!(order_step(2, 5, middle(3) - 1.), None);
        assert_eq!(order_step(2, 5, middle(3) + 1.), Some(4));
        assert_eq!(order_step(2, 5, middle(1) - 1.), Some(1));
        // Down from the last but one, past the last.
        assert_eq!(order_step(3, 5, middle(4) + 1.), Some(5));
        // Never before the fixed first place, nor past the end.
        assert_eq!(order_step(1, 5, -100.), None);
        assert_eq!(order_step(4, 5, 1000.), None);
    }
}
