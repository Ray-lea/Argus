//! Route table long presses to Argus's row and header menus before the
//! component's empty context menu or a header tooltip claims the gesture.
use super::*;

#[derive(Clone)]
pub(in crate::frontend) struct Target {
    page: String,
    row: Option<String>,
    column: Option<String>,
}

impl Desktop {
    pub(super) fn table_touch(&self, cx: &Context<Self>) -> AnyElement {
        let owner = cx.entity().downgrade();
        let target = self.ui.table_touch.clone();
        canvas(
            |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
            move |_, hitbox, window, _| {
                window.on_mouse_event({
                    let owner = owner.clone();
                    let hitbox = hitbox.clone();
                    let target = target.clone();
                    move |event: &TouchDragEvent, phase, window, cx| {
                        if phase != DispatchPhase::Capture || event.phase != TouchPhase::Started {
                            return;
                        }
                        *target.borrow_mut() = None;
                        if !hitbox.is_hovered(window) {
                            return;
                        }
                        let Some(app) = owner.upgrade() else { return };
                        let app = app.read(cx);
                        let row = app
                            .table
                            .as_ref()
                            .and_then(|table| table.read(cx).delegate().list.as_ref())
                            .and_then(|list| {
                                app.tips.row_under(list, ROW_HEIGHT, app.rows.len(), window)
                            })
                            .map(|index| app.rows[index].key.clone());
                        let column = (app.page == "processes")
                            .then(|| {
                                app.header_bounds
                                    .borrow()
                                    .iter()
                                    .find(|(_, bounds)| bounds.contains(&event.start_position))
                                    .map(|(key, _)| key.clone())
                            })
                            .flatten();
                        *target.borrow_mut() = Some(Target {
                            page: app.page.clone(),
                            row,
                            column,
                        });
                    }
                });
                window.on_mouse_event(move |event: &LongPressEvent, phase, window, cx| {
                    if phase != DispatchPhase::Capture
                        || event.phase != TouchPhase::Started
                        || !hitbox.is_hovered(window)
                    {
                        return;
                    }
                    let Some(app) = owner.upgrade() else { return };
                    let app = app.read(cx);
                    let Some(target) = target
                        .borrow()
                        .clone()
                        .filter(|target| target.page == app.page)
                    else {
                        return;
                    };
                    if target.row.is_none() && target.column.is_none() {
                        return;
                    }
                    window.prevent_default();
                    cx.stop_propagation();
                    let owner = owner.clone();
                    window.defer(cx, move |window, cx| {
                        let _ = owner.update(cx, |this, cx| {
                            if this.page != target.page {
                                return;
                            }
                            if let Some(key) = target.row {
                                if let Some(row) = this.rows.iter().position(|row| row.key == key) {
                                    this.open_row_menu(row, window, cx);
                                }
                            } else {
                                this.process_header_menu(target.column, None, window, cx);
                            }
                        });
                    });
                });
            },
        )
        .absolute()
        .inset_0()
        .into_any_element()
    }
}
