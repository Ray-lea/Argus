//! The columns' widths: the grid tracks they resolve to, the columns gliding
//! to new widths and a list's columns dragged to a width.
use super::*;

/// The interface's preference that keeps the widths `page`'s columns were
/// dragged to, by column.
pub(in crate::frontend) fn list_widths_preference(page: &str) -> String {
    format!("tableWidths.{page}")
}
pub(super) struct TableLayout<'a> {
    pub(super) available: Pixels,
    pub(super) columns: &'a columns::Preferences,
    pub(super) totals: HashMap<String, String>,
    pub(super) bits: bool,
    pub(super) scope: (&'a str, &'a str),
    /// The process list the automatic column widths measure (`WidthRows`).
    pub(super) measured: Option<Arc<Vec<WidthRow>>>,
    /// A service or startup list as loaded, which its column widths
    /// measure, and the widths its columns were dragged to (`fills`).
    pub(super) entries: Option<&'a [Value]>,
    pub(super) list_widths: BTreeMap<String, f32>,
}
/// A column easing from one width to another.
#[derive(Clone, Copy)]
pub(super) struct WidthGlide {
    from: f32,
    to: f32,
    started: Instant,
}
impl WidthGlide {
    fn progress(&self, now: Instant) -> f32 {
        now.saturating_duration_since(self.started).as_secs_f32() / widths::RESIZE.as_secs_f32()
    }
    fn at(&self, now: Instant) -> f32 {
        let progress = self.progress(now);
        // The curve lands a hair off either end: its ends are taken exactly.
        if progress <= 0. {
            self.from
        } else if progress >= 1. {
            self.to
        } else {
            self.from + (self.to - self.from) * theme::ease_move(progress)
        }
    }
}
impl NativeTable {
    /// The service, startup and connection tables size their fixed columns for
    /// Chinese and English; a longer header or state label in the current
    /// language widens its column instead of being cut off. Measured once per
    /// language and text size.
    fn fit_fixed(&mut self, page: &str, fields: &mut [Field], cx: &App) {
        let system = gpui_kit::gpui::WindowTextSystem::new(cx.text_system().clone());
        let width = |text: &str, size: f32| {
            let run = theme::text_run(text, theme::LABEL_WEIGHT, cx);
            f32::from(
                system
                    .layout_line(text, theme::font(size), &[run], None)
                    .width,
            )
            .ceil()
        };
        for field in fields {
            let Track::Fixed(designed) = field.track else {
                continue;
            };
            let fitted = *self
                .fitted
                .entry((page.to_owned(), field.key))
                .or_insert_with(|| {
                    // The header leaves room for the sort arrow and a
                    // filter's button, as `AutoWidths` does.
                    let header = width(&field.label, 12.) + widths::header_extra(page, field.key);
                    let (prefix, keys, beside) = cell_labels(field.kind);
                    let labels = keys.iter().map(|key| format!("{prefix}.{key}"));
                    // A startup entry's switch and its gap before the label.
                    let (labels, beside): (Vec<String>, f32) = if field.kind == CellKind::Enabled {
                        (
                            [
                                "common.enabled",
                                "common.disabled",
                                "startup.stateUnavailable",
                            ]
                            .map(str::to_owned)
                            .into(),
                            28. + 8.,
                        )
                    } else {
                        (labels.collect(), beside)
                    };
                    // A start type can also show as pending a restart, which is longer.
                    let pending = (field.kind == CellKind::StartType)
                        .then(|| keys.iter().map(|key| pending_start_type(key)))
                        .into_iter()
                        .flatten();
                    labels
                        .iter()
                        .map(|key| label(key))
                        .chain(pending)
                        .map(|text| width(&text, 13.) + beside + CELL_PADDING * 2.)
                        .fold(header, f32::max)
                        .ceil()
                });
            field.track = Track::Fixed(designed.max(fitted));
        }
    }
    pub(super) fn configure(
        &mut self,
        page: &str,
        rows: Arc<Vec<Row>>,
        sources: columns::Sources,
        layout: TableLayout<'_>,
        sort: (&str, bool),
        cx: &App,
    ) -> bool {
        let TableLayout {
            available,
            columns: preferences,
            totals,
            bits,
            scope,
            measured,
            entries,
            list_widths,
        } = layout;
        let rows_changed = !Arc::ptr_eq(&self.rows, &rows);
        let had_rows = !self.rows.is_empty();
        // An empty list (none sampled yet) measures nothing.
        let measured = measured.filter(|measured| !measured.is_empty());
        let measured_changed = measured
            .as_ref()
            .is_some_and(|measured| !Arc::ptr_eq(&self.measured, measured));
        let totals_changed = self.totals != totals;
        let locale = argus_core::i18n::locale();
        let byte_unit = ByteUnit::current();
        self.cell_values.configure(&rows, page, bits, cx);
        if rows_changed || self.page != page {
            self.group_view.groups = Rc::new(if page == "processes" {
                groups::groups(&rows)
            } else {
                Vec::new()
            });
        }
        self.rows = rows;
        self.totals = totals;
        self.bits = bits;
        if self.page == page && !self.fields.is_empty() && cx.has_active_drag() {
            return false;
        }
        let mut fields = if page == "processes" {
            process_fields(preferences, sources)
        } else {
            fields(page, sources, f32::from(available))
        };
        let font_size = theme::font(12.);
        let same_layout = self.page == page
            && self.available == available
            && self.font_size == font_size
            && self.locale == locale
            && self.byte_unit == byte_unit
            && self.scope.0 == scope.0
            && self.scope.1 == scope.1
            && (page != "processes" || &self.process_columns == preferences)
            && self.list_widths == list_widths
            && self
                .fields
                .iter()
                .map(|f| (&f.key, &f.label))
                .eq(fields.iter().map(|f| (&f.key, &f.label)));
        let gliding = self
            .horizontal
            .as_ref()
            .is_some_and(|h| h.wheel_target() != h.offset());
        // Whether the widths follow the rows, which only these tables' do.
        let contents = if page == "processes" {
            preferences.auto_width
        } else {
            fills(page)
        };
        if same_layout
            && (!contents || (!rows_changed && !totals_changed && !measured_changed) || gliding)
        {
            return false;
        }
        // A column whose contents or settings ask for another width eases
        // there; a new window size, text size, language or byte unit takes
        // effect at once, as do a list's first rows.
        let restyled =
            self.font_size != font_size || self.locale != locale || self.byte_unit != byte_unit;
        let animate = (page == "processes" || (fills(page) && had_rows))
            && self.page == page
            && !self.columns.is_empty()
            && self.available == available
            && !restyled
            && !cx.reduce_motion();
        let shown: HashMap<&'static str, f32> = self
            .fields
            .iter()
            .zip(&self.columns)
            .map(|(field, column)| (field.key, f32::from(column.width)))
            .collect();
        // Measured widths last through window sizes, views, pages and column
        // order: only another text size, language or byte unit measures them anew.
        if restyled {
            self.auto_widths.reset();
            self.fitted.clear();
        }
        self.locale = locale;
        self.byte_unit = byte_unit;
        self.scope = (scope.0.into(), scope.1.into());
        self.available = available;
        self.font_size = font_size;
        self.process_columns = preferences.clone();
        let page_changed = self.page != page;
        self.page = page.into();
        self.list_widths = list_widths;
        if page == "processes" && preferences.auto_width {
            self.measured = match measured {
                Some(measured) => measured,
                // Until the list is built, the rows shown stand in for it.
                None if rows_changed || self.measured.is_empty() => Arc::new(
                    self.rows
                        .iter()
                        .map(|row| widths::WidthRow::of(row, &fields, bits))
                        .collect(),
                ),
                None => self.measured.clone(),
            };
            let (first, count) = self
                .list
                .as_ref()
                .map(|list| {
                    let height = f32::from(list.bounds().size.height);
                    (
                        (f32::from(-list.offset().y).max(0.) / ROW_HEIGHT) as usize,
                        if height > 0. {
                            (height / ROW_HEIGHT).ceil() as usize + 4
                        } else {
                            64
                        },
                    )
                })
                .unwrap_or((0, 64));
            self.auto_widths.apply(
                &fields,
                widths::Contents {
                    rows: &self.rows,
                    range: first..(first + count.min(96)).min(self.rows.len()),
                    measured: &self.measured,
                    totals: &self.totals,
                    values: &mut self.cell_values,
                    bits,
                },
                cx,
            );
            // The columns share the table's room by their contents
            // (`widths::allocate`). A column dragged to a width keeps it and
            // takes no part in the sharing: the others keep their widths, so
            // the drag moves only what is right of it and nothing slides back
            // once it is let go. The rows sit 4 px in from each side and the
            // scrollbar keeps its gutter.
            let room = f32::from(available) - ROW_INSET * 2. - CELL_PADDING;
            let naturals = fields
                .iter()
                .map(|field| {
                    self.auto_widths
                        .natural
                        .get(field.key)
                        .copied()
                        .unwrap_or_else(|| {
                            widths::Natural::fixed(match field.track {
                                Track::Fixed(width)
                                | Track::Flex(width, _)
                                | Track::Fit(width, _)
                                | Track::Fill(width) => width,
                            })
                        })
                })
                .collect::<Vec<_>>();
            let mut allocated = widths::allocate(&naturals, room);
            // Full width: the name column takes the room the others leave,
            // unless it was dragged to a width of its own.
            if preferences.full_width && preferences.set_width("name").is_none() {
                let others: f32 = allocated.iter().skip(1).sum();
                if let Some(name) = allocated.first_mut() {
                    *name = name.max((room - others).floor());
                }
            }
            for (field, width) in fields.iter_mut().zip(allocated) {
                field.track = Track::Fixed(preferences.set_width(field.key).unwrap_or(width));
            }
        } else if fills(page) {
            self.fit_fixed(page, &mut fields, cx);
            // Each list as loaded is measured, not the rows a search or a
            // filter leaves of it: neither changes the widths.
            if let Some(entries) = entries
                && (rows_changed || restyled || page_changed || self.auto_widths.natural.is_empty())
            {
                self.auto_widths.apply_list(page, &fields, entries, cx);
            }
            self.list_shares = fields.iter().map(|field| self.share(field)).collect();
            let (widths, _) = self.list_layout(&fields, None);
            for (field, width) in fields.iter_mut().zip(widths) {
                field.track = Track::Fixed(width);
            }
        } else if page != "processes" {
            self.fit_fixed(page, &mut fields, cx);
        }
        let changed = !same_layout
            || !self
                .fields
                .iter()
                .map(|field| field.track)
                .eq(fields.iter().map(|field| field.track));
        self.fields = fields;
        if !changed {
            return false;
        }
        // Resolve the grid tracks like CSS: fixed tracks first, then the flexible
        // ones share what is left by their fractions without going below their minimum.
        // The rows sit 4 px in from each side and the scrollbar keeps its gutter.
        let room = f32::from(available) - ROW_INSET * 2. - CELL_PADDING;
        // A list's tracks are fixed by now (`widths::fill`).
        let base = |track: Track| match track {
            Track::Fixed(width) | Track::Fit(width, _) | Track::Fill(width) => width,
            Track::Flex(minimum, _) => minimum,
        };
        let fixed: f32 = self
            .fields
            .iter()
            .filter(|f| !matches!(f.track, Track::Flex(..)))
            .map(|f| base(f.track))
            .sum();
        let mut flexible: Vec<usize> = (0..self.fields.len())
            .filter(|&i| matches!(self.fields[i].track, Track::Flex(..)))
            .collect();
        let mut widths: Vec<f32> = self.fields.iter().map(|f| base(f.track)).collect();
        let mut free = (room - fixed).max(0.);
        loop {
            let fractions: f32 = flexible
                .iter()
                .map(|&i| match self.fields[i].track {
                    Track::Flex(_, fr) => fr,
                    _ => 0.,
                })
                .sum();
            if fractions <= 0. {
                break;
            }
            let unit = free / fractions;
            // A track whose share falls under its minimum keeps the minimum and
            // leaves the distribution, like `minmax()` in a grid.
            let clamped: Vec<usize> = flexible
                .iter()
                .copied()
                .filter(|&i| match self.fields[i].track {
                    Track::Flex(minimum, fr) => unit * fr < minimum,
                    _ => false,
                })
                .collect();
            if clamped.is_empty() {
                for &i in &flexible {
                    if let Track::Flex(_, fr) = self.fields[i].track {
                        widths[i] = unit * fr;
                    }
                }
                break;
            }
            for i in clamped {
                free -= widths[i];
                flexible.retain(|&f| f != i);
            }
            free = free.max(0.);
        }
        // A column bound for another width glides there from where it is
        // drawn now; one already on its way to that width keeps going.
        let now = Instant::now();
        if animate {
            for (field, &target) in self.fields.iter().zip(&widths) {
                if self
                    .width_glides
                    .get(field.key)
                    .is_some_and(|glide| glide.to == target)
                {
                    continue;
                }
                let from = self
                    .width_glides
                    .get(field.key)
                    .map(|glide| glide.at(now))
                    .or_else(|| shown.get(field.key).copied());
                match from {
                    Some(from) if (from - target).abs() >= 0.5 => {
                        self.width_glides.insert(
                            field.key,
                            WidthGlide {
                                from,
                                to: target,
                                started: now,
                            },
                        );
                    }
                    _ => {
                        self.width_glides.remove(field.key);
                    }
                }
            }
            self.width_glides
                .retain(|key, _| self.fields.iter().any(|field| field.key == *key));
        } else {
            self.width_glides.clear();
        }
        // A list's last column spans the room the others leave: it has no
        // edge to drag, and the others are dragged only as far wider as the
        // columns can give way (`widths::fill`).
        let list = fills(page).then(|| {
            let last = self
                .list_shares
                .iter()
                .position(|share| matches!(share, widths::Share::Fill { .. }));
            (last, self.list_layout(&self.fields, None).1)
        });
        self.columns = self
            .fields
            .iter()
            .zip(widths)
            .enumerate()
            .map(|(index, (field, target))| {
                let width = self
                    .width_glides
                    .get(field.key)
                    .map_or(target, |glide| glide.at(now));
                let column = Column::new(field.key, field.label.clone());
                let (column, resizable) = match &list {
                    Some((last, reach)) => (
                        column.max_width(
                            (target + reach.get(index).copied().unwrap_or(0.)).max(width),
                        ),
                        *last != Some(index),
                    ),
                    None => (column, true),
                };
                // Process columns can be dragged to a width with automatic
                // widths on as well: that column keeps it, the others still
                // follow their contents.
                let column = column
                    .width(width)
                    .min_width(48.)
                    .resizable(resizable)
                    .movable(page == "processes" && field.key != "name")
                    .paddings(Edges::all(px(0.)))
                    .sort(if field.key == sort.0 {
                        if sort.1 {
                            ColumnSort::Descending
                        } else {
                            ColumnSort::Ascending
                        }
                    } else {
                        ColumnSort::Default
                    });
                if index == 0 {
                    column.fixed(ColumnFixed::Left)
                } else if field.end {
                    column.text_right()
                } else {
                    column
                }
            })
            .collect();
        true
    }
    /// Steps the columns gliding to new widths. Returns whether a width
    /// changed, and whether any column is still on its way.
    pub(super) fn glide_widths(&mut self, now: Instant) -> (bool, bool) {
        if self.width_glides.is_empty() {
            return (false, false);
        }
        let mut changed = false;
        for (field, column) in self.fields.iter().zip(self.columns.iter_mut()) {
            if let Some(glide) = self.width_glides.get(field.key) {
                let width = px(glide.at(now));
                if column.width != width {
                    column.width = width;
                    changed = true;
                }
            }
        }
        self.width_glides
            .retain(|_, glide| glide.progress(now) < 1.);
        (changed, !self.width_glides.is_empty())
    }
    /// Marks process column `key` as set back to its automatic width, or
    /// fitted to its contents with automatic widths off; returns the width
    /// that fits them then.
    pub(in crate::frontend) fn restore_column(
        &mut self,
        key: &str,
        auto: bool,
        cx: &App,
    ) -> Option<f32> {
        let key = self.fields.iter().find(|field| field.key == key)?.key;
        self.mark_restored(key);
        if auto { None } else { self.fit_width(key, cx) }
    }
    /// How list column `field` shares the room before it is dragged to a
    /// width (`widths::fill`), from what its contents need.
    fn share(&self, field: &Field) -> widths::Share {
        let natural = self.auto_widths.natural.get(field.key).copied();
        match field.track {
            Track::Fit(minimum, _) if matches!(field.kind, CellKind::Name | CellKind::Dim) => {
                widths::Share::Text {
                    natural: natural.unwrap_or(widths::Natural::fixed(minimum)),
                    min: minimum,
                    set: false,
                }
            }
            Track::Fit(minimum, _) => {
                widths::Share::Fixed(natural.map_or(minimum, |natural| natural.full))
            }
            Track::Fill(minimum) => widths::Share::Fill {
                full: natural.map_or(minimum, |natural| natural.full),
                min: minimum,
            },
            Track::Fixed(width) | Track::Flex(width, _) => widths::Share::Fixed(width),
        }
    }
    /// The widths of a list's `fields` for the table's room
    /// (`widths::fill`), each column dragged to a width kept at it, and how
    /// far wider each can be dragged. `dragged` is a column being dragged,
    /// at the width it has reached. A width kept from an earlier drag gives
    /// way, as text does, when the room is short (a narrower window), down
    /// to what the column takes before it is dragged.
    fn list_layout(&self, fields: &[Field], dragged: Option<(&str, f32)>) -> (Vec<f32>, Vec<f32>) {
        let shares = fields
            .iter()
            .zip(&self.list_shares)
            .map(|(field, share)| match (*share, dragged) {
                (widths::Share::Fill { .. }, _) => *share,
                (_, Some((key, width))) if key == field.key => widths::Share::Fixed(width),
                (share, _) => match self.list_widths.get(field.key) {
                    Some(&width) => widths::Share::Text {
                        natural: widths::Natural::fixed(width),
                        min: width.min(match share {
                            widths::Share::Text { min, .. } => min,
                            widths::Share::Fixed(width) => width,
                            widths::Share::Fill { min, .. } => min,
                        }),
                        set: true,
                    },
                    None => share,
                },
            })
            .collect::<Vec<_>>();
        widths::fill(
            &shares,
            f32::from(self.available) - ROW_INSET * 2. - CELL_PADDING,
        )
    }
    /// A list column's edge being dragged (the table's width for it has
    /// moved off the column's): the columns share the room as they will once
    /// it is let go, so the last column follows the edge. Returns whether
    /// the columns changed, for the table to take them up (`refresh`).
    pub(super) fn follow_list_drag(state: &mut TableState<Self>) -> bool {
        let delegate = state.delegate();
        if !fills(&delegate.page) || delegate.list_shares.len() != delegate.columns.len() {
            return false;
        }
        let Some((index, width)) =
            delegate
                .columns
                .iter()
                .enumerate()
                .find_map(|(index, column)| {
                    let width = state.col_width(index)?;
                    (column.resizable && (width - column.width).abs() >= px(0.5))
                        .then_some((index, f32::from(width)))
                })
        else {
            return false;
        };
        let delegate = state.delegate_mut();
        let key = delegate.fields[index].key;
        delegate.list_dragged = Some(key);
        let (widths, reach) = delegate.list_layout(&delegate.fields, Some((key, width)));
        delegate.width_glides.clear();
        for ((column, width), reach) in delegate.columns.iter_mut().zip(widths).zip(reach) {
            column.width = px(width);
            column.max_width = px(width + reach);
        }
        true
    }
    /// What process column `key` takes to show every row in full: its width
    /// when fitted to its contents.
    pub(in crate::frontend) fn fit_width(&mut self, key: &str, cx: &App) -> Option<f32> {
        let field = self.fields.iter().find(|field| field.key == key)?.clone();
        Some(self.auto_widths.fit(
            &field,
            widths::Contents {
                rows: &self.rows,
                range: 0..0,
                measured: &self.measured,
                totals: &self.totals,
                values: &mut self.cell_values,
                bits: self.bits,
            },
            cx,
        ))
    }
}
/// Case-insensitive comparison that orders digit runs by value, like `Intl.Collator`
/// with `numeric: true`.
impl Desktop {
    /// A service or startup list as loaded, which its column widths measure.
    pub(super) fn list_entries(&self, page: &str) -> Option<&[Value]> {
        fills(page)
            .then(|| self.data.get(page).map(values))
            .flatten()
    }
    /// The widths `page`'s columns were dragged to (`list_widths_preference`).
    pub(in crate::frontend) fn list_widths(&self, page: &str) -> BTreeMap<String, f32> {
        if !fills(page) {
            return BTreeMap::new();
        }
        let mut widths: BTreeMap<String, f32> = self
            .prefs
            .get(&list_widths_preference(page), BTreeMap::new());
        widths.retain(|_, width| width.is_finite() && (48. ..=1200.).contains(width));
        widths
    }
    /// List column `key` follows its contents again, as before it was dragged.
    pub(in crate::frontend) fn restore_list_column(
        &mut self,
        page: &str,
        key: &str,
        cx: &mut Context<Self>,
    ) {
        let mut widths = self.list_widths(page);
        if widths.remove(key).is_some() {
            self.prefs.set(&list_widths_preference(page), widths);
            cx.notify();
        }
    }
    pub(in crate::frontend) fn process_columns_diagnostics(&self, cx: &App) -> Value {
        // The page's table where it sizes its columns by their contents,
        // else the process list's.
        let page = if fills(&self.page) {
            self.page.as_str()
        } else {
            "processes"
        };
        let table = self.tables.get(page).map(|pair| &pair.0);
        let visible=table.map(|table| {
            let state=table.read(cx);
            state.delegate().fields.iter().enumerate().map(|(index,field)|json!({
                "key":field.key,"width":state.col_width(index).map(f32::from),
                "maxWidth":f32::from(state.delegate().columns[index].max_width),
                "resizable":state.delegate().columns[index].resizable,
                "movable":state.delegate().columns[index].movable,
                "autoWidth":state.delegate().auto_widths.natural.get(field.key).map(|natural|natural.typical),
                "fullWidth":state.delegate().auto_widths.natural.get(field.key).map(|natural|natural.full),
                "gliding":state.delegate().width_glides.contains_key(field.key),
                "bounds":self.header_bounds.borrow().get(field.key).map(|b|[f32::from(b.left()),f32::from(b.top()),f32::from(b.size.width),f32::from(b.size.height)])
            })).collect::<Vec<_>>()
        }).unwrap_or_default();
        let anchors = self
            .ui
            .anchors
            .borrow()
            .iter()
            .filter(|(key, _)| key.starts_with("column-"))
            .map(|(key, b)| {
                (
                    key.to_string(),
                    json!([
                        f32::from(b.left()),
                        f32::from(b.top()),
                        f32::from(b.size.width),
                        f32::from(b.size.height)
                    ]),
                )
            })
            .collect::<serde_json::Map<_, _>>();
        let scroll = table.map(|table| {
            let state = table.read(cx);
            let handle = state.horizontal_scroll_handle.as_ref();
            json!({"x":f32::from(handle.offset().x),"maxX":f32::from(handle.max_offset().x),
                "y":f32::from(state.vertical_scroll_handle.0.borrow().base_handle.offset().y)})
        });
        // The groups, the headings held at the top and the name beside the scrollbar.
        let groups = table.map(|table| {
            let delegate = table.read(cx).delegate();
            delegate.group_view.diagnostics(&delegate.rows)
        });
        json!({"page":page,"listWidths":self.list_widths(page),
            "preferences":columns::Preferences::load(&self.prefs),"visible":visible,"anchors":anchors,
            "scroll":scroll,"sort":{"key":self.sort,"desc":self.descending},"groups":groups,
            "popup":self.popup.is_some(),"modal":self.modal.as_ref().filter(|modal|modal.kind=="columns").map(|modal|&modal.args)})
    }

    pub(in crate::frontend) fn capture_process_widths(
        &self,
        preferences: &mut columns::Preferences,
        cx: &App,
    ) {
        let table = self
            .tables
            .get("processes")
            .map(|pair| &pair.0)
            .or_else(|| {
                (self.page == "processes")
                    .then_some(self.table.as_ref())
                    .flatten()
            });
        if let Some(table) = table {
            let state = table.read(cx);
            for (index, field) in state.delegate().fields.iter().enumerate() {
                if let Some(width) = state.col_width(index) {
                    preferences
                        .widths
                        .insert(field.key.into(), f32::from(width).clamp(48., 1200.));
                }
            }
        }
    }
}
