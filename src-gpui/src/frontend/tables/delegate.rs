//! The component table's delegate (`NativeTable`): the state behind a table,
//! its header and its cells as the component lays them out.
use super::*;

pub(in crate::frontend) struct NativeTable {
    pub(super) owner: WeakEntity<Desktop>,
    pub(super) page: String,
    pub(super) rows: Arc<Vec<Row>>,
    pub(super) fields: Vec<Field>,
    pub(super) columns: Vec<Column>,
    pub(super) available: Pixels,
    pub(super) process_columns: columns::Preferences,
    pub(super) font_size: Pixels,
    pub(super) locale: argus_core::i18n::Locale,
    pub(super) byte_unit: ByteUnit,
    pub(super) bits: bool,
    pub(super) scope: (String, String),
    pub(super) totals: HashMap<String, String>,
    pub(super) auto_widths: widths::AutoWidths,
    /// The process list the automatic widths measure (`WidthRows`); the
    /// rows shown stand in until there is one.
    pub(super) measured: Arc<Vec<WidthRow>>,
    /// Fixed column widths of the other tables for the current language and
    /// text size, by page and column (`fit_fixed`).
    pub(super) fitted: HashMap<(String, &'static str), f32>,
    /// How a service or startup list's columns share the room before any
    /// is dragged to a width (`widths::fill`), and the widths they were
    /// dragged to (`fills`).
    pub(super) list_shares: Vec<widths::Share>,
    pub(super) list_widths: BTreeMap<String, f32>,
    /// The list column whose edge is being dragged; its width is kept once
    /// it is let go.
    pub(super) list_dragged: Option<&'static str>,
    pub(super) cell_values: widths::CellValues,
    pub(super) horizontal: Option<ScrollHandle>,
    /// A row the desktop selected itself; the table's select event for it is ignored.
    pub(super) echo: Option<usize>,
    /// Cell hints (`tips`): the row under the pointer reports its hinted
    /// parts, read from the list's scroll handle once this render lays out rows.
    pub(super) tips: tips::Tips,
    pub(super) list: Option<ScrollHandle>,
    /// The list's handle as the rows are scrolled to with (`body`).
    pub(super) vertical: Option<UniformListScrollHandle>,
    /// The view that draws the rows Argus draws (`body::TableBody`).
    pub(super) body: Option<Entity<body::TableBody>>,
    /// Whether Argus draws this frame's rows (`body`), settled by the frame
    /// before the table renders; the component's table then has none.
    pub(super) own_rows: bool,
    /// The row the table selected last, which the keys go on from (`body`).
    pub(super) selected_row: Option<usize>,
    pub(super) near: Option<usize>,
    pub(super) near_due: bool,
    /// Why disk readings are missing, as of a sample: bytes, then requests.
    pub(super) disk_hint: RefCell<[Option<(u64, String)>; 2]>,
    /// A process header being dragged: its column follows the pointer while
    /// the others make room, gliding to their new places (`flip`).
    pub(super) header_drag: Option<HeaderDrag>,
    /// Columns gliding to new places after the dragged one passed them.
    pub(super) flips: flip::Flips,
    /// The header band, which clips gliding header cells.
    pub(super) header_band: Rc<Cell<Bounds<Pixels>>>,
    /// The column raised under the pointer, until it has glided into place.
    pub(super) lifted: Option<&'static str>,
    /// Process columns on their way to a new width (`TableFrame` steps them).
    pub(super) width_glides: HashMap<&'static str, WidthGlide>,
    /// The process column whose right edge is under the pointer, and since
    /// when (`header::track_edge`).
    pub(super) edge_hover: Option<usize>,
    pub(super) edge_since: Option<Instant>,
    /// A column just set back to its automatic width, and when (`header::restore_wash`).
    pub(super) restored: Option<(&'static str, Instant)>,
    /// The column whose edge is being dragged to another width.
    pub(super) resizing: Option<usize>,
    /// A process column a double-click on its edge just fitted, until the
    /// next single press (`fit_column`). The table drags no edge meanwhile,
    /// so the widths it reports then are its rounding of the fit, not a
    /// width to keep.
    pub(super) edge_fitted: Option<&'static str>,
    /// The process list's user and category groups (`groups`).
    pub(super) group_view: groups::GroupView,
    /// The column under the pointer when a row was last right-pressed,
    /// for the menu it opens (`Desktop::open_row_menu` takes it).
    pub(in crate::frontend) context_column: Option<&'static str>,
}
impl NativeTable {
    pub(super) fn new(owner: WeakEntity<Desktop>, tips: tips::Tips) -> Self {
        Self {
            owner,
            tips,
            list: None,
            vertical: None,
            body: None,
            own_rows: false,
            selected_row: None,
            near: None,
            near_due: false,
            disk_hint: RefCell::default(),
            header_drag: None,
            flips: Default::default(),
            header_band: Rc::new(Cell::new(Bounds::default())),
            lifted: None,
            width_glides: HashMap::new(),
            edge_hover: None,
            edge_since: None,
            restored: None,
            resizing: None,
            edge_fitted: None,
            group_view: Default::default(),
            context_column: None,
            page: String::new(),
            rows: Arc::new(vec![]),
            fields: vec![],
            columns: vec![],
            available: px(0.),
            process_columns: columns::Preferences::default(),
            font_size: px(0.),
            locale: argus_core::i18n::locale(),
            byte_unit: ByteUnit::current(),
            bits: false,
            scope: (String::new(), String::new()),
            totals: HashMap::new(),
            auto_widths: Default::default(),
            measured: Arc::new(vec![]),
            fitted: HashMap::new(),
            list_shares: Vec::new(),
            list_widths: BTreeMap::new(),
            list_dragged: None,
            cell_values: Default::default(),
            horizontal: None,
            echo: None,
        }
    }
}
impl TableDelegate for NativeTable {
    /// A double-click on a process column's edge: with automatic widths on,
    /// a column dragged to a width follows its contents again; with them
    /// off, the column fits its contents. Either glides there (`configure`)
    /// rather than jumping, so the table is answered with the width the
    /// column has now. Answering at all makes the table ignore drags on that
    /// edge until the next single press: the double-click's second press
    /// starts one, and a slight move would set a dragged width again.
    fn fit_column(
        &mut self,
        col_ix: usize,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> Option<Pixels> {
        // A list column dragged to a width follows its contents again.
        if fills(&self.page) {
            let key = self.fields.get(col_ix)?.key;
            if self.list_widths.contains_key(key) {
                self.mark_restored(key);
                cx.notify();
            }
            self.edge_fitted = Some(key);
            let (owner, page) = (self.owner.clone(), self.page.clone());
            window.defer(cx, move |_, cx| {
                let _ = owner.update(cx, |this, cx| this.restore_list_column(&page, key, cx));
            });
            return self.columns.get(col_ix).map(|column| column.width);
        }
        if self.page != "processes" {
            return None;
        }
        let key = self.fields.get(col_ix)?.key;
        let width = if self.process_columns.auto_width {
            None
        } else {
            self.fit_width(key, cx)
        };
        if !self.process_columns.auto_width || self.process_columns.overrides.contains_key(key) {
            self.mark_restored(key);
            cx.notify();
        }
        // The table rounds the answer up to whole device pixels and reports
        // the widths when that changes the column: not a width to keep.
        self.edge_fitted = Some(key);
        let owner = self.owner.clone();
        window.defer(cx, move |_, cx| {
            let _ = owner.update(cx, |this, cx| this.fit_process_column(key, width, cx));
        });
        self.columns.get(col_ix).map(|column| column.width)
    }
    fn move_column(
        &mut self,
        from: usize,
        to: usize,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) {
        if self.page != "processes"
            || from == 0
            || to == 0
            || from >= self.fields.len()
            || to >= self.fields.len()
        {
            return;
        }
        let field = self.fields.remove(from);
        self.fields.insert(to, field);
        let column = self.columns.remove(from);
        self.columns.insert(to, column);
    }
    fn columns_count(&self, _cx: &App) -> usize {
        self.columns.len()
    }
    /// None while Argus draws the rows: the component then shows the body
    /// in their place (`render_empty`).
    fn rows_count(&self, _cx: &App) -> usize {
        if self.own_rows { 0 } else { self.rows.len() }
    }
    fn column(&self, index: usize, _cx: &App) -> Column {
        self.columns[index].clone()
    }
    fn perform_sort(
        &mut self,
        index: usize,
        sort: ColumnSort,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) {
        let key = self.fields[index].key.to_owned();
        if !sortable(&self.page, &key) {
            return;
        }
        let owner = self.owner.clone();
        window.defer(cx, move |_, cx| {
            let _ = owner.update(cx, |this, cx| {
                this.sort = key;
                this.descending = sort == ColumnSort::Descending;
                if let Some((key, value)) = sort_preference(&this.page, &this.sort, this.descending)
                {
                    this.prefs.set(&key, value);
                }
                this.rebuild_rows(false, cx);
                cx.notify();
            });
        });
    }
    fn render_header(
        &mut self,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> Stateful<Div> {
        // The rows render after the header, laid out where the list scrolled
        // to this frame: the first one settles which reports its hints. The
        // rows Argus draws do this themselves, in frames of their own too
        // (`body`).
        let own_body = self.own_rows && !self.rows.is_empty();
        if !own_body {
            self.near_due = self.tips.begin(window);
            self.near = None;
        }
        // A search narrows the process list: an accent header with the card's
        // accent ring (`.workspace.filtered .thead`), rounded into the card's
        // corner, and the right one too while the details are closed.
        let filtered = (self.page == "processes")
            .then(|| self.owner.upgrade())
            .flatten()
            .map(|owner| {
                let app = owner.read(cx);
                (!app.query.trim().is_empty(), app.details_shown())
            });
        let p = Palette::current(cx);
        let now = Instant::now();
        // A drag released away from the header (no drop here) settles too.
        if self.header_drag.is_some() && !cx.has_active_drag() {
            self.settle_header_drag(!cx.reduce_motion(), now);
        }
        // Columns that have just come to rest: the frame settles again who
        // draws the rows, which it did a moment before (`body`).
        let settled = !self.own_rows && self.draws_rows(now, window);
        if self.header_drag.is_some()
            || self.flips.active(now)
            || self.restored.is_some()
            || settled
        {
            window.request_animation_frame();
        }
        let band = self.header_band.clone();
        let strip = self.lifted_strip(now, cx);
        // Column edges show where they can be dragged (`header::edge_probe`).
        let processes = self.page == "processes";
        let edges = processes || fills(&self.page);
        let guide = edges.then(|| self.restore_wash(now, cx)).flatten();
        let probe = edges.then(|| self.edge_probe(cx));
        // The process header band ends in its own separator row, so the 4 px
        // below the labels are inside it (see `render_th`): no bottom padding.
        // While filtering, the workspace draws the accent separator over that
        // row (`process_header_height`).
        div()
            .id("native-table-header")
            .relative()
            .on_prepaint(move |bounds, _, _| band.set(bounds))
            .children(strip)
            .children(guide)
            .children(probe)
            .on_drag_move(
                cx.listener(|state, event: &DragMoveEvent<HeaderPayload>, window, cx| {
                    Self::follow_header(state, event, window, cx)
                }),
            )
            .on_drop(cx.listener(|state, _: &HeaderPayload, _, cx| {
                let animate = !cx.reduce_motion();
                state
                    .delegate_mut()
                    .settle_header_drag(animate, Instant::now());
                cx.notify();
            }))
            .when(self.page == "processes", |d| {
                let owner = self.owner.clone();
                d.on_mouse_down(MouseButton::Right, move |event, window, cx| {
                    cx.stop_propagation();
                    let owner = owner.clone();
                    let at = event.position;
                    window.defer(cx, move |window, cx| {
                        let _ = owner.update(cx, |this, cx| {
                            // The column under the pointer, for its own items.
                            let column = this
                                .header_bounds
                                .borrow()
                                .iter()
                                .find(|(_, bounds)| bounds.contains(&at))
                                .map(|(key, _)| key.clone());
                            this.process_header_menu(column, None, window, cx)
                        });
                    });
                })
            })
            // The process columns' edges in the header are the rows' own:
            // the rows have no inset, so neither does the header (the line
            // on an edge and a dragged column's strip are drawn by them).
            .when(!processes, |d| d.px(px(ROW_INSET)))
            .pt(px(4.))
            .when(filtered.is_none(), |d| d.pb(px(4.)))
            .when_some(filtered.filter(|(on, _)| *on), |d, (_, details)| {
                d.bg(p.accent.opacity(0.08))
                    .rounded_tl(px(7.))
                    .when(!details, |d| d.rounded_tr(px(7.)))
            })
    }
    /// The space after the last column: the columns leave room for the
    /// header's insets and the scrollbar gutter (see `configure`), and with the
    /// library's 12 px default here the header would scroll sideways by 4 px
    /// even when the columns fit, showing a horizontal thumb.
    fn render_last_empty_col(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        div().w(px(ROW_INSET * 2.)).h_full().flex_shrink_0()
    }
    fn render_empty(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        // The rows Argus draws, where the component has none of its own.
        if self.own_rows
            && !self.rows.is_empty()
            && let Some(body) = &self.body
        {
            return body::TableBody::element(body);
        }
        let p = Palette::current(cx);
        let owner = self.owner.clone();
        let (message, clear) = self
            .owner
            .upgrade()
            .map(|owner| owner.read(cx).empty_message())
            .unwrap_or_default();
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_3()
            .text_color(p.muted)
            .child(message)
            .when_some(clear, |d, text| {
                d.child(
                    Button::new("table-clear-filter")
                        .px_3()
                        .h(px(32.))
                        .rounded(px(4.))
                        .border_1()
                        .border_color(p.stroke)
                        .bg(p.control)
                        .text_color(p.ink)
                        .hover(move |s| s.bg(p.hover))
                        .child(text)
                        .on_click(move |_, window, cx| {
                            let owner = owner.clone();
                            window.defer(cx, move |window, cx| {
                                let _ = owner.update(cx, |this, cx| {
                                    if matches!(this.page.as_str(), "processes" | "network") {
                                        this.prefs.set(visibility::preference(&this.page), false);
                                    }
                                    this.preference("clearSearch", Value::Null, window, cx);
                                    let page = this.page.clone();
                                    this.clear_filters(&page, cx);
                                });
                            });
                        }),
                )
            })
            .into_any_element()
    }
    fn render_th(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let field = &self.fields[index];
        let p = Palette::current(cx);
        let end = field.end;
        // Process headers: the interface's 4 px cell padding inside its 4 px
        // header padding below the label, which the header band leaves out.
        let processes = self.page == "processes";
        let filter = self.header_filter(index, cx);
        // The table keeps its own inset at the right of a header's content
        // (`widths::HEADER_INSET`): a process header leaves out as much of its
        // padding, so its figures end where the cells' do. A filter button
        // takes the end of its header.
        let mut cell = div()
            .size_full()
            .pl(px(CELL_PADDING))
            .pr(px(if processes {
                CELL_PADDING - widths::HEADER_INSET
            } else if filter.is_some() {
                FILTER_BUTTON + 4.
            } else {
                CELL_PADDING
            }))
            .pt(px(4.))
            .pb(px(if processes { 8. } else { 4. }))
            .flex()
            .flex_col()
            .justify_end()
            .when(end, |d| d.items_end())
            .when(!end, |d| d.items_start());
        if self.page != "processes" {
            cell = cell.justify_center();
        }
        if self.page == "processes"
            && let Some(owner) = self.owner.upgrade()
        {
            let app = owner.read(cx);
            let total = self.totals.get(field.key).cloned().unwrap_or_default();
            // Leak alerts: a mark after the count, which lists them.
            let alerts = if field.key == "name" {
                app.leak_alerts().len()
            } else {
                0
            };
            let leak_mark = (alerts > 0).then(|| {
                let tip = theme::counted("processes.leak.mark", alerts as u64, &[]);
                let owner = self.owner.clone();
                let tint = p.io;
                div()
                    .id(leaks::MARK)
                    .relative()
                    .flex()
                    .flex_shrink_0()
                    .items_center()
                    .gap(px(3.))
                    .h(px(18.))
                    .px(px(6.))
                    .rounded_full()
                    .border_1()
                    .border_color(tint.opacity(0.45))
                    .bg(tint.opacity(0.1))
                    .text_color(tint)
                    .text_size(theme::font(11.))
                    .line_height(px(16.))
                    .font_family(cx.theme().font_family.clone())
                    .font_weight(FontWeight::SEMIBOLD)
                    .cursor_pointer()
                    .hover(move |s| s.bg(tint.opacity(0.18)))
                    .child(icon("WarningIcon", tint).size(px(11.)))
                    .child(theme::count(alerts as f64))
                    .child(layout::anchor_probe(leaks::MARK, app.ui.anchors.clone()))
                    // Its press never reaches the header, which would sort.
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(move |_, window, cx| {
                        cx.stop_propagation();
                        let owner = owner.clone();
                        window.defer(cx, move |window, cx| {
                            let _ = owner.update(cx, |this, cx| this.open_leak_menu(window, cx));
                        });
                    })
                    .tooltip(move |window, cx| widgets::tooltip(tip.clone(), window, cx))
            });
            cell = cell.child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .text_size(theme::font(15.))
                    .line_height(relative(1.25))
                    .font_family(theme::display_font(cx))
                    .text_color(p.ink)
                    .whitespace_nowrap()
                    .max_w_full()
                    .overflow_hidden()
                    .when(field.key == "name" && !app.query.trim().is_empty(), |d| {
                        let tip = label("processes.filtered");
                        d.child(
                            div()
                                .id("filter-mark")
                                .flex()
                                .flex_shrink_0()
                                .child(icon("FilterIcon", p.accent).size(px(13.)))
                                .tooltip(move |window, cx| {
                                    widgets::tooltip(tip.clone(), window, cx)
                                }),
                        )
                    })
                    .child(raised_brackets(total, theme::font(15.)))
                    .children(leak_mark),
            );
        }
        let owner = self.owner.clone();
        let key = field.key.to_owned();
        let sort = owner
            .upgrade()
            .map(|owner| {
                let app = owner.read(cx);
                (app.sort == key, app.descending, app.held_order)
            })
            .unwrap_or_default();
        let label = div()
            .flex()
            .items_center()
            .gap(px(2.))
            .min_w_0()
            .max_w_full()
            .text_size(theme::font(12.))
            .line_height(relative(1.4))
            .when(processes, |d| d.mt(px(1.)))
            .whitespace_nowrap()
            .text_color(if sort.0 { p.ink } else { p.muted })
            .when(sort.0, |d| d.font_weight(theme::LABEL_WEIGHT))
            .child(div().min_w_0().truncate().child(field.label.clone()))
            .when(sort.0 && !sort.2, |d| {
                d.child(
                    icon(if sort.1 { "ChevronDown" } else { "ChevronUp" }, p.ink)
                        .size(px(12.))
                        .flex_shrink_0(),
                )
            })
            // Rows held out of this order (`Desktop::holds_order`): the
            // arrow gives way to a pause mark in the accent until they re-sort.
            .when(sort.0 && sort.2, |d| {
                let tip = theme::label("processes.orderHeld");
                d.child(
                    div()
                        .id("order-held-mark")
                        .flex()
                        .flex_shrink_0()
                        .child(icon("PauseIcon", p.accent).size(px(12.)))
                        .tooltip(move |window, cx| {
                            widgets::wrapping_tooltip(tip.clone().into(), px(320.), window, cx)
                        }),
                )
            });
        let hover = p.hover;
        let press = p.press;
        let header_bounds = owner
            .upgrade()
            .map(|owner| owner.read(cx).header_bounds.clone());
        let measured_key = key.clone();
        // A column the table cannot sort by has a disabled header: no
        // hover or press fill, and a click does nothing (`.th:disabled`).
        let sortable = sortable(&self.page, &key);
        // While a column edge is dragged, the headers it passes over stay
        // quiet: no hover fill and no description.
        let resizing = self.resizing.is_some();
        let button = Button::new(("column-sort", index))
            .size_full()
            .min_w_0()
            .rounded(px(4.))
            .bg(gpui_kit::gpui::transparent_black())
            .on_prepaint(move |bounds, _, _| {
                if let Some(headers) = &header_bounds {
                    headers.borrow_mut().insert(measured_key.clone(), bounds);
                }
            })
            .when(sortable && !resizing, |button| {
                button
                    .hover(move |s| s.bg(hover))
                    .active(move |s| s.bg(press))
            })
            .disabled(!sortable)
            .accessibility_label(field.label.clone())
            .when(self.page == "processes" && !resizing, |button| {
                // The memory header's figure is whole-system usage, its rows
                // private working sets: the hint names both.
                let title = theme::label(&if key == "mem" {
                    "processes.memoryScope.header".to_owned()
                } else {
                    format!("processes.column.{key}.title")
                });
                // Wrapped like the cells' tips: some descriptions run to a few sentences.
                button.tooltip(move |window, cx| {
                    widgets::wrapping_tooltip(title.clone().into(), px(420.), window, cx)
                })
            })
            .child(cell.child(label))
            .on_click(move |_, window, cx| {
                let owner = owner.clone();
                let key = key.clone();
                window.defer(cx, move |window, cx| {
                    let _ = owner.update(cx, |this, cx| this.act(Action::Sort(key), window, cx));
                });
            });
        let header = self.header_cell(index, button, window, cx);
        match filter {
            Some(filter) => div()
                .relative()
                .size_full()
                .child(header)
                .child(filter)
                .into_any_element(),
            None => header,
        }
    }
    fn render_td(
        &mut self,
        row_index: usize,
        column: usize,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let content = self.td_content(row_index, column, window, cx);
        let key = self.fields.get(column).map(|field| field.key);
        // The row menu offers to copy the cell under the pointer. Rows Argus
        // draws note it themselves (`body`); here the cell does, before the
        // row's own right press opens the menu.
        div()
            .size_full()
            .min_w_0()
            .flex()
            .items_center()
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |state, _: &MouseDownEvent, _, _| {
                    state.delegate_mut().context_column = key;
                }),
            )
            .child(self.gliding_cell(column, false, content))
    }
    fn render_tr(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> Stateful<Div> {
        if std::mem::take(&mut self.near_due) {
            self.near = self.list.as_ref().and_then(|list| {
                self.tips
                    .row_under(list, ROW_HEIGHT, self.rows.len(), window)
                    // Under a held heading, the row is out of sight.
                    .filter(|_| !self.group_view.covers(list, window))
            });
        }
        let p = Palette::current(cx);
        let tint = self
            .group_view
            .tone(index)
            .map(|tone| tone.color(&p).opacity(groups::HEADING_TINT));
        let row = &self.rows[index];
        let (selected, fresh, searching) = self
            .owner
            .upgrade()
            .map(|owner| {
                let app = owner.read(cx);
                (
                    app.selected
                        .get(&self.page)
                        .is_some_and(|key| *key == row.key),
                    self.page == "processes" && app.is_fresh(row),
                    !app.query.trim().is_empty(),
                )
            })
            .unwrap_or_default();
        // A heading cannot fold while a search shows every match; its row says so.
        let locked = (row.section.is_some() && searching && self.is_near(index))
            .then(|| label("processes.sectionSearching"));
        // Pinned rows lead the list; a line under the last one ends them.
        let last_pinned = row.pinned && self.rows.get(index + 1).is_some_and(|next| !next.pinned);
        // A double-click folds a group (the table reports the first, click
        // count 2). Another double-click right after it arrives as clicks 3 and
        // 4 within the system's double-click time and would be lost until that
        // time passed: every completed pair folds again. A category heading
        // folds on every press instead (its select event), as the interface's
        // `onClick`: no pair folds it again.
        let folds = matches!(self.page.as_str(), "processes" | "network");
        let refold = (folds && row.expandable && row.section.is_none()).then(|| {
            let owner = self.owner.clone();
            let key = row.key.clone();
            move |event: &ClickEvent, window: &mut Window, cx: &mut App| {
                let count = event.click_count();
                if count >= 4 && count.is_multiple_of(2) {
                    let owner = owner.clone();
                    let key = key.clone();
                    window.defer(cx, move |window, cx| {
                        let _ =
                            owner.update(cx, |this, cx| this.act(Action::Fold(key), window, cx));
                    });
                }
            }
        });
        let tr = div()
            .id(("native-row", index))
            .relative()
            .border_b_0()
            .rounded(px(4.));
        let tr = match locked {
            Some(hint) => self
                .tips
                .target(tr, tips::id((&self.page, &row.key, "row")), hint),
            None => tr,
        };
        tr.when_some(refold, |d, refold| d.on_click(refold))
            // A group's heading in its colour (`groups`).
            .when_some(tint, |d, tint| d.bg(tint))
            .when(fresh, |d| d.bg(p.network.opacity(0.15)))
            .when(selected, |d| {
                d.child(
                    div()
                        .absolute()
                        .left_0()
                        .top(px(8.))
                        .bottom(px(8.))
                        .w(px(3.))
                        .rounded(px(2.))
                        .bg(p.accent),
                )
            })
            .when(last_pinned, |d| {
                d.child(
                    div()
                        .absolute()
                        .left(px(8.))
                        .right(px(8.))
                        .bottom(px(-1.))
                        .h(px(1.))
                        .bg(p.stroke_strong),
                )
            })
    }
    // Row menus open from the desktop (see `RightClickedRow`), where submenus can nest.
    fn context_menu(
        &mut self,
        _index: usize,
        menu: PopupMenu,
        _window: &mut Window,
        _cx: &mut Context<TableState<Self>>,
    ) -> PopupMenu {
        menu
    }
}
/// How far the brackets in a process header's figures are raised, in em.
/// The display font has no case-sensitive forms: its brackets reach below
/// the baseline as they do around lowercase text, and hang under the
/// figures and the Chinese text between them. Raised, they centre on both.
const BRACKET_RAISE: f32 = 0.08;
/// `text` set in `size` with its brackets raised (`BRACKET_RAISE`).
fn raised_brackets(text: String, size: Pixels) -> AnyElement {
    if !text.contains(['(', ')']) {
        return text.into_any_element();
    }
    let mut row = div().flex();
    let mut start = 0;
    for (at, bracket) in text.match_indices(['(', ')']) {
        if start < at {
            row = row.child(text[start..at].to_owned());
        }
        row = row.child(
            div()
                .relative()
                .top(-size * BRACKET_RAISE)
                .child(bracket.to_owned()),
        );
        start = at + bracket.len();
    }
    if start < text.len() {
        row = row.child(text[start..].to_owned());
    }
    row.into_any_element()
}
