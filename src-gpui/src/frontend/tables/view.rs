//! The table's view: the frame around the component's table, the region the
//! desktop draws it in, and the selection the table mirrors.
use super::*;

pub(in crate::frontend) struct TableFrame {
    state: Entity<TableState<NativeTable>>,
}
impl TableFrame {
    /// What a key that moves the selected row does (`NativeTable::step`).
    fn step<A: 'static>(&self, step: body::Step) -> impl Fn(&A, &mut Window, &mut App) + 'static {
        let state = self.state.clone();
        move |_, _, cx| {
            cx.stop_propagation();
            state.update(cx, |state, cx| NativeTable::step(state, step, cx));
        }
    }
}
impl Render for TableFrame {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Columns on their way to a new width take a step each frame. Not
        // while a header is dragged: the table's widths are the drag's then,
        // and nothing steps, so no frames are asked for. The drag draws the
        // window again as the pointer moves and once more when it ends, which
        // takes the glides up again.
        let now = Instant::now();
        // Who draws this frame's rows (`body`), settled before the table
        // counts them. A new theme draws the table without configuring its
        // columns: the cells' styles follow it here, before anything of this
        // frame shows a cell (the held headings below among them).
        let own_rows = self.state.update(cx, |state, cx| {
            let delegate = state.delegate_mut();
            delegate.cell_values.prepare(cx);
            delegate.own_rows = delegate.draws_rows(now, window);
            delegate.own_rows && !delegate.rows.is_empty()
        });
        let gliding = self.state.update(cx, |state, cx| {
            if cx.has_active_drag() {
                // A list's last column follows a dragged edge as it moves.
                if NativeTable::follow_list_drag(state) {
                    state.refresh(cx);
                }
                return false;
            }
            let (changed, gliding) = state.delegate_mut().glide_widths(now);
            if changed {
                state.refresh(cx);
                cx.notify();
            }
            gliding
        });
        if gliding {
            window.request_animation_frame();
        }
        // The process list's held headings and its groups beside the scrollbar
        // (`groups`): planned before the table renders, from where it is
        // scrolled. The rows Argus draws come with their own (`body`).
        let groups = self.state.update(cx, |state, cx| {
            let delegate = state.delegate_mut();
            if own_rows || delegate.page != "processes" {
                return None;
            }
            delegate.plan_groups(window, cx);
            delegate.group_overlay(process_header_band(), window, cx)
        });
        // Process headers carry a total above the label; the other tables use the
        // 34 px header cell. Both sit in a 4 px padded header row.
        let state = self.state.read(cx);
        let rows = state.vertical_scroll_handle.0.borrow().base_handle.clone();
        let table = self.state.entity_id();
        let dark = cx.theme().mode == ThemeMode::Dark;
        let processes = state.delegate().page == "processes";
        let columns = state.horizontal_scroll_handle.as_ref().clone();
        use gpui_kit::base::actions::{
            SelectDown, SelectFirst, SelectLast, SelectPageDown, SelectUp,
        };
        let bottom = if processes { scrollbar::SIZE } else { 0. };
        div()
            .relative()
            .size_full()
            // The process table keeps a rail for sideways scrolling below its
            // rows, even when the columns fit (`.hscroll`).
            .when(processes, |d| d.pb(px(scrollbar::SIZE)))
            // The keys that move the selection among rows the component's
            // table does not have, taken before it sees them (`body`).
            .when(own_rows, |d| {
                d.capture_action::<SelectUp>(self.step(body::Step::Up))
                    .capture_action::<SelectDown>(self.step(body::Step::Down))
                    .capture_action::<SelectFirst>(self.step(body::Step::First))
                    .capture_action::<SelectLast>(self.step(body::Step::Last))
                    .capture_action::<SelectPageDown>(self.step(body::Step::PageDown))
            })
            .child(scrollbar::wheel_glide(&rows, &columns, table))
            .child(
                DataTable::new(&self.state)
                    .small()
                    .header_row_height(if processes {
                        process_header_band()
                    } else {
                        px(34.) + theme::text_growth(12., 1.4)
                    })
                    .bordered(false)
                    .fixed_cols_border(false)
                    .stripe(false)
                    // The interface's scrollbars are drawn below.
                    .scrollbar_visible(false, false),
            )
            // The component's table takes the wheel all over itself for its
            // rows, which keeps it from an ancestor's scrolling; with the
            // rows Argus draws it has none to do that for.
            .when(own_rows, |d| {
                d.child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .right_0()
                        .bottom(px(bottom))
                        .child(gpui_kit::base::ScrollableMask::new(Axis::Vertical, &rows)),
                )
            })
            .children(groups)
            // The process page draws the list's bar itself (`Desktop::table_scrollbar`).
            .when(!processes && !own_rows, |d| {
                d.child(scrollbar::scrollbar("table-scrollbar", &rows, dark).view(table))
            })
            .when(processes, |d| {
                d.child(
                    scrollbar::scrollbar("table-columns-scrollbar", &columns, dark)
                        .rail_below(&rows)
                        .view(table),
                )
            })
    }
}
/// Shows the desktop's selection in the table without the side effects of a
/// click: no select event back to the desktop (which would toggle it off) and no
/// scroll to the row, which would pull the list back whenever rows reorder.
fn mirror_selection(
    state: &mut TableState<NativeTable>,
    index: usize,
    cx: &mut Context<TableState<NativeTable>>,
) {
    state.delegate_mut().echo = Some(index);
    state.set_selected_row(index, cx);
    state
        .vertical_scroll_handle
        .0
        .borrow_mut()
        .deferred_scroll_to_item = None;
}
impl Desktop {
    /// Lays the current page's table out for its rows and the column widths.
    pub(super) fn configure_table(&mut self, cx: &mut Context<Self>) {
        // Apply data before rendering. Updating TableState from table_view
        // invalidates its parent during paint and rebuilds Desktop twice per sample.
        if let Some(table) = &self.table {
            let rows = self.rows.clone();
            let page = self.page.clone();
            let sources = columns::Sources::of(&self.snapshot);
            let sort = (self.sort.as_str(), self.descending);
            let column_preferences = columns::Preferences::load(&self.prefs);
            let totals = self.process_column_totals(&column_preferences);
            let bits = self.prefs.get::<String>("netUnit", "bytes".into()) == "bits";
            let scope = (self.view.as_str(), self.query.as_str());
            let measured = self.width_rows.as_ref().map(|built| built.rows.clone());
            let entries = self.list_entries(&page);
            let list_widths = self.list_widths(&page);
            let selected = self
                .selected
                .get(&page)
                .and_then(|key| rows.iter().position(|row| &row.key == key));
            table.update(cx, |state, cx| {
                let available = state.delegate().available;
                if available <= px(0.) {
                    return;
                }
                if state.delegate_mut().configure(
                    &page,
                    rows,
                    sources,
                    TableLayout {
                        available,
                        columns: &column_preferences,
                        totals,
                        bits,
                        scope,
                        measured,
                        entries,
                        list_widths,
                    },
                    sort,
                    cx,
                ) {
                    state.refresh(cx);
                }
                if selected != state.selected_row() {
                    if let Some(index) = selected {
                        mirror_selection(state, index, cx);
                    } else {
                        state.clear_selection(cx);
                    }
                }
                cx.notify();
            });
        }
        if let Some(frame) = &self.table_frame {
            frame.update(cx, |_, cx| cx.notify());
        }
    }
    /// The list's vertical scrollbar for a page that draws it beside the table:
    /// the process page, whose list is drawn narrower than it is laid out while
    /// the details pane opens or closes.
    pub(in crate::frontend) fn table_scrollbar(&self, cx: &App) -> Option<scrollbar::Scrollbar> {
        let table = self.table.as_ref()?;
        let rows = table
            .read(cx)
            .vertical_scroll_handle
            .0
            .borrow()
            .base_handle
            .clone();
        Some(
            scrollbar::scrollbar("table-scrollbar", &rows, self.palette.dark)
                .view(table.entity_id()),
        )
    }
    /// Where the list draws the row at `index`, scrolled as it is: where a
    /// test presses on it (see `Tips::row_under` for the same measure).
    pub(in crate::frontend) fn row_bounds(&self, index: usize, cx: &App) -> Option<Bounds<Pixels>> {
        let list = self.table.as_ref()?.read(cx).delegate().list.clone()?;
        let viewport = list.bounds();
        (index < self.rows.len()).then(|| {
            Bounds::new(
                point(
                    viewport.left(),
                    viewport.top() + list.offset().y + px(ROW_HEIGHT * index as f32),
                ),
                size(viewport.size.width, px(ROW_HEIGHT)),
            )
        })
    }
    pub(in crate::frontend) fn table_view(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.table.is_none() {
            let owner = cx.entity().downgrade();
            let tips = self.tips.clone();
            let table = cx.new(|cx| {
                let mut state = TableState::new(NativeTable::new(owner, tips), window, cx)
                    .col_selectable(false)
                    .sortable(false)
                    // Process headers are dragged by the table itself (`HeaderDrag`).
                    .col_movable(false);
                let list = state.vertical_scroll_handle.0.borrow().base_handle.clone();
                let horizontal = state.horizontal_scroll_handle.as_ref().clone();
                // A wheel glide draws only the table frame (the nearest cached
                // view around the rows) in each frame; the regions and the
                // desktop around it replay. What reads the offsets outside
                // the frame asks for its own frames (`scrollbar::Scrollbar`).
                list.isolate_glides();
                horizontal.isolate_glides();
                state.delegate_mut().vertical = Some(state.vertical_scroll_handle.clone());
                state.delegate_mut().list = Some(list);
                state.delegate_mut().horizontal = Some(horizontal);
                state
            });
            self.subscriptions.push(cx.subscribe_in(
                &table,
                window,
                |this, source, event, window, cx| match event {
                    TableEvent::MoveColumn(_, to) => {
                        let state = source.read(cx);
                        if state.delegate().page != "processes" {
                            return;
                        }
                        let fields = &state.delegate().fields;
                        let Some(moved) = fields.get(*to) else {
                            return;
                        };
                        let mut preferences = columns::Preferences::load(&this.prefs);
                        if preferences
                            .move_before(moved.key, fields.get(to + 1).map(|field| field.key))
                        {
                            this.set_process_columns(preferences, cx);
                        }
                    }
                    TableEvent::ColumnWidthsChanged(widths) => {
                        // A list column let go after a drag keeps its width.
                        let dragged = source.update(cx, |state, _| {
                            let delegate = state.delegate_mut();
                            let key = delegate.list_dragged.take()?;
                            let index = delegate.fields.iter().position(|f| f.key == key)?;
                            Some((delegate.page.clone(), key, f32::from(*widths.get(index)?)))
                        });
                        if let Some((page, key, width)) = dragged {
                            let mut saved = this.list_widths(&page);
                            saved.insert(key.into(), width.round().clamp(48., 1200.));
                            this.prefs.set(&list_widths_preference(&page), saved);
                            cx.notify();
                            return;
                        }
                        let source = source.read(cx);
                        let delegate = source.delegate();
                        // Not a drag: a double-click just fitted a column.
                        if delegate.page != "processes" || delegate.edge_fitted.is_some() {
                            return;
                        }
                        let mut preferences = columns::Preferences::load(&this.prefs);
                        for ((field, column), width) in
                            delegate.fields.iter().zip(&delegate.columns).zip(widths)
                        {
                            let width = f32::from(*width).clamp(48., 1200.);
                            if !preferences.auto_width {
                                preferences.widths.insert(field.key.into(), width);
                            } else if (f32::from(column.width) - width).abs() >= 0.5 {
                                // Only the column dragged: the others keep
                                // following their contents.
                                preferences.overrides.insert(field.key.into(), width);
                            }
                        }
                        this.prefs.set(columns::PREFERENCE, preferences);
                        cx.notify();
                    }
                    TableEvent::SelectRow(index) => {
                        let echo = this.table.as_ref().is_some_and(|table| {
                            table.update(cx, |state, _| state.delegate_mut().echo.take())
                                == Some(*index)
                        });
                        if echo {
                            return;
                        }
                        if let Some(row) = this.rows.get(*index).cloned() {
                            if row.section.is_some() {
                                this.act(Action::Fold(row.key.clone()), window, cx);
                                this.sync_table_selection(cx);
                            } else {
                                this.act(Action::Select(row.key.clone()), window, cx);
                            }
                        }
                    }
                    TableEvent::DoubleClickedRow(index) => {
                        if let Some(row) = this.rows.get(*index) {
                            let row = row.clone();
                            if this.page == "processes" {
                                // A heading already folded on this press (`SelectRow`).
                                if row.expandable && row.section.is_none() {
                                    this.act(Action::Fold(row.key.clone()), window, cx);
                                }
                            } else {
                                this.activate_row(&row, window, cx);
                            }
                        }
                    }
                    TableEvent::RightClickedRow(Some(index)) => {
                        this.open_row_menu(*index, window, cx);
                    }
                    TableEvent::ClearSelection => {
                        // A selection the current filter or folding hides stays
                        // selected; only clearing a visible row deselects it.
                        if this
                            .selected
                            .get(&this.page)
                            .is_some_and(|key| this.rows.iter().any(|row| &row.key == key))
                        {
                            this.selected.remove(&this.page);
                        }
                        cx.notify();
                    }
                    _ => {}
                },
            ));
            let body = cx.new(|_| body::TableBody::new(table.downgrade()));
            table.update(cx, |state, _| state.delegate_mut().body = Some(body));
            self.table = Some(table);
            let state = self.table.as_ref().expect("table created").clone();
            self.table_frame = Some(cx.new(|_| TableFrame { state }));
            self.tables.insert(
                self.page.clone(),
                (
                    self.table.as_ref().unwrap().clone(),
                    self.table_frame.as_ref().unwrap().clone(),
                ),
            );
        }
        let table = self.table.as_ref().expect("table initialized").clone();
        let rows = self.rows.clone();
        let page = self.page.clone();
        let sources = columns::Sources::of(&self.snapshot);
        let sort = (self.sort.as_str(), self.descending);
        let column_preferences = columns::Preferences::load(&self.prefs);
        let bits = self.prefs.get::<String>("netUnit", "bytes".into()) == "bits";
        let scope = (self.view.as_str(), self.query.as_str());
        let measured = self.width_rows.as_ref().map(|built| built.rows.clone());
        let list_widths = self.list_widths(&page);
        let available = window.viewport_size().width
            - px(24.)
            - px(if self.page == "processes" && self.details_shown() {
                self.details_display_width(window)
            } else {
                0.
            })
            - px(if self.nav_top {
                0.
            } else {
                self.side_nav_width()
            });
        let selected = self
            .selected
            .get(&page)
            .and_then(|key| rows.iter().position(|r| &r.key == key));
        // Whether the table is laid out anew. The column totals read every
        // process and only that uses them: a frame that scrolls the list or
        // moves a highlight leaves them alone.
        let stale = {
            let state = table.read(cx);
            let delegate = state.delegate();
            !Arc::ptr_eq(&delegate.rows, &rows)
                || delegate.available != available
                || (page == "processes" && delegate.process_columns != column_preferences)
                || delegate.list_widths != list_widths
                || delegate.font_size != theme::font(12.)
                || delegate.bits != bits
                || delegate.locale != argus_core::i18n::locale()
                || delegate.byte_unit != ByteUnit::current()
        };
        let totals = if stale {
            self.process_column_totals(&column_preferences)
        } else {
            HashMap::new()
        };
        let entries = self.list_entries(&page);
        let changed = table.update(cx, |state, cx| {
            let mut changed = false;
            // A column dragged to a width is drawn at it already: a glide to
            // the saved width starts there, not where the drag began.
            if !cx.has_active_drag() {
                for index in 0..state.delegate().columns.len() {
                    if let Some(width) = state.col_width(index) {
                        state.delegate_mut().columns[index].width = width;
                    }
                }
            }
            if stale {
                if state.delegate_mut().configure(
                    &page,
                    rows,
                    sources,
                    TableLayout {
                        available,
                        columns: &column_preferences,
                        totals,
                        bits,
                        scope,
                        measured,
                        entries,
                        list_widths,
                    },
                    sort,
                    cx,
                ) {
                    state.refresh(cx);
                }
                changed = true;
            }
            if selected != state.selected_row() {
                changed = true;
                if let Some(index) = selected {
                    mirror_selection(state, index, cx);
                } else {
                    state.clear_selection(cx);
                }
            }
            if changed {
                cx.notify();
            }
            changed
        });
        if changed && let Some(frame) = &self.table_frame {
            frame.update(cx, |_, cx| cx.notify());
        }
        let tips = self.tips.clone();
        div()
            .id("table-region")
            .size_full()
            // A row under the pointer takes its fill in place: the list does
            // not render for the pointer moving over it. What a row shows for
            // the pointer otherwise asks for its own render (`tips`).
            .hover_fills_in_place()
            .on_hover(cx.listener(|this, hovered, window, cx| {
                this.pointer_in_table = *hovered;
                if !hovered {
                    this.tips.dismiss(window, cx);
                    // Only the process list holds its order under the pointer.
                    if this.page == "processes" {
                        this.rebuild_rows(false, cx);
                    }
                }
                // Outside the list, only the status bar says the order is
                // held: the desktop draws its own tree again. Rows put back
                // in order told the list themselves (`install_rows`).
                window.request_view_redraw(cx.entity_id());
            }))
            // Cell hints follow the pointer here, one listener for the table.
            .on_mouse_move(move |event, window, cx| tips.moved(event, window, cx))
            .child(self.table_touch(cx))
            .child(
                self.table_frame
                    .as_ref()
                    .expect("table frame")
                    .clone()
                    .cached(StyleRefinement::default().size_full()),
            )
            .into_any_element()
    }
    /// Puts the table's own selection back on the selected row (or none), after
    /// a click the desktop did not turn into a selection.
    pub(in crate::frontend) fn sync_table_selection(&self, cx: &mut Context<Self>) {
        let selected = self
            .selected
            .get(&self.page)
            .and_then(|key| self.rows.iter().position(|row| &row.key == key));
        if let Some(table) = &self.table {
            table.update(cx, |state, cx| {
                match selected {
                    Some(index) => mirror_selection(state, index, cx),
                    None => state.clear_selection(cx),
                }
                state.set_right_clicked_row(None, cx);
            });
        }
    }
    /// The column a right press on a row was over, once: the menu it opens
    /// takes it.
    pub(in crate::frontend) fn take_context_column(&self, cx: &mut App) -> Option<&'static str> {
        self.table
            .as_ref()?
            .update(cx, |state, _| state.delegate_mut().context_column.take())
    }
    /// What an empty table says, and the label of the button that clears its filter.
    pub(in crate::frontend) fn empty_message(&self) -> (String, Option<String>) {
        let query = self.query.trim();
        if matches!(self.page.as_str(), "processes" | "network")
            && self.prefs.get(visibility::preference(&self.page), false)
        {
            return (
                label("processes.visibility.empty"),
                Some(label("processes.visibility.clear")),
            );
        }
        match self.page.as_str() {
            "processes" => (
                tr!("processes.noMatch", query = query),
                Some(label("shell.search.clear")),
            ),
            "services"
                if self
                    .data
                    .get("services")
                    .is_none_or(|s| values(s).is_empty()) =>
            {
                (label("services.loading"), None)
            }
            "services" => (
                label("services.noMatch"),
                Some(label("services.clearFilter")),
            ),
            "startup" if self.data.get("startup").is_none_or(Value::is_null) => {
                (label("startup.loading"), None)
            }
            "startup" => (label("startup.noMatch"), None),
            "network" if self.connections.as_deref().is_none_or(Value::is_null) => {
                (label("connections.loading"), None)
            }
            "network" => (
                if !query.is_empty() && query.bytes().all(|b| b.is_ascii_digit()) {
                    tr!("connections.noMatchPort", port = query)
                } else {
                    label("connections.noMatch")
                },
                Some(label("connections.clearFilter")),
            ),
            _ => (String::new(), None),
        }
    }
}
