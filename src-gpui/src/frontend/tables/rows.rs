//! The rows a table shows: built from the sample, held in their places under
//! the pointer, and marked with their notes and pins.
use super::*;

/// The note and pin of the row marked `key`.
pub(super) fn row_mark(marks: &Value, key: &str) -> (Option<String>, bool) {
    (
        marks[key]["note"].as_str().map(str::to_owned),
        boolean(&marks[key], "pinned"),
    )
}
/// The icon a row shows: a WebView's host application's, else its own.
/// The interface's preference: whether the pointer over the process list
/// holds its order (on unless turned off).
pub(in crate::frontend) const HOLD_ORDER: &str = "holdOrderOnHover";
impl Desktop {
    /// Whether the process list holds its order: the pointer is over it (as
    /// `HOLD_ORDER` allows), or a menu or dialog is open. Its rows keep their
    /// places while samples arrive and groups fold or unfold, until neither holds.
    pub(in crate::frontend) fn holds_order(&self) -> bool {
        (self.pointer_in_table && self.hover_holds_order())
            || self.modal.is_some()
            || self.popup.is_some()
    }
    /// Whether the pointer over the process list holds its order (`HOLD_ORDER`).
    pub(in crate::frontend) fn hover_holds_order(&self) -> bool {
        self.prefs.get(HOLD_ORDER, true)
    }
    pub(in crate::frontend) fn rebuild_rows(&mut self, hold: bool, cx: &mut Context<Self>) {
        self.rebuild_rows_prepared(hold, None, cx);
    }
    pub(in crate::frontend) fn rebuild_rows_prepared(
        &mut self,
        hold: bool,
        prepared: Option<Vec<Row>>,
        cx: &mut Context<Self>,
    ) {
        let started = Instant::now();
        let mut rows = if let Some(rows) = prepared {
            rows
        } else if self.page == "processes" {
            let users = self.user_groups();
            model::process_rows(
                values(&self.snapshot["processes"]),
                model::ProcessOptions {
                    mode: &self.view,
                    query: &self.query,
                    sort: &self.sort,
                    descending: self.descending,
                    expanded: &self.expanded,
                    tree_expanded: &self.tree_expanded,
                    sections: &self.sections,
                    hosted: &self.hosted_services(),
                    visibility: self.visibility_rules("processes").as_ref(),
                    users: users.as_ref(),
                    user_folds: &self.user_folds,
                    open: false,
                },
            )
        } else {
            let page = self.page.clone();
            let spec = self.list_spec(&page, first_column(&page));
            if page == "network" {
                let (source, owners) = self.connection_source();
                lists::list_rows(
                    &spec,
                    source.as_deref().map(values).unwrap_or_default(),
                    &owners,
                )
            } else {
                let source = match page.as_str() {
                    "services" | "startup" => self.data.get(&page).map(values),
                    _ => None,
                };
                lists::list_rows(&spec, source.unwrap_or_default(), &HashMap::new())
            }
        };
        // The other lists come with their marks, pinned rows first.
        if self.page == "processes" {
            self.mark_rows(&mut rows);
            // A search, a fold or another order leaves the measured list as
            // it is; another view or grouping builds it again.
            self.refresh_width_rows(None, cx);
        }
        if hold && self.page == "processes" {
            let order: HashMap<_, _> = self
                .rows
                .iter()
                .enumerate()
                .map(|(i, r)| (&r.key, i))
                .collect();
            // Rows keep their places; a new row follows the row it comes after in
            // the fresh order, so it stays in its category instead of dropping to the end.
            let fresh: Vec<String> = rows.iter().map(|r| r.key.clone()).collect();
            let mut place = -1.;
            let mut keyed: Vec<(f64, Row)> = rows
                .into_iter()
                .map(|row| {
                    place = match order.get(&row.key) {
                        Some(&index) => index as f64,
                        None => place + 1e-6,
                    };
                    (place, row)
                })
                .collect();
            keyed.sort_by(|a, b| a.0.total_cmp(&b.0));
            rows = keyed.into_iter().map(|(_, row)| row).collect();
            self.held_order = !rows.iter().map(|r| &r.key).eq(fresh.iter());
        } else {
            self.held_order = false;
        }
        let rows = Arc::new(rows);
        self.install_rows(rows.clone(), cx);
        if self.page == "network" {
            self.keep_connection_rows(rows);
        }
        super::trace_work(
            "rows",
            Some(&format!("{} {}", self.page, self.rows.len())),
            started,
        );
        let elapsed = started.elapsed().as_micros() as u64;
        self.work.rows_us += elapsed;
        self.work.rows_max_us = self.work.rows_max_us.max(elapsed);
        self.work.row_builds += 1;
    }
    /// Gives process rows the note and pin of their row marks.
    fn mark_rows(&self, rows: &mut [Row]) {
        let marks = self.marks();
        for row in rows {
            if let Some(key) = &row.mark_key {
                (row.note, row.pinned) = row_mark(&marks, key);
            }
        }
    }
    /// Shows `rows` on the current page's table.
    pub(in crate::frontend) fn install_rows(
        &mut self,
        rows: Arc<Vec<Row>>,
        cx: &mut Context<Self>,
    ) {
        self.rows_epoch = self.rows_epoch.wrapping_add(1);
        if rows.len() != self.rows.len() {
            self.list
                .reset_with_uniform_height(rows.len(), px(ROW_HEIGHT));
        }
        let previous_rows = std::mem::replace(&mut self.rows, rows);
        self.configure_table(cx);
        // Retain old rows until delegates release them, then reclaim the value trees off-thread.
        if !previous_rows.is_empty() {
            cx.background_executor()
                .spawn(async move { drop(previous_rows) })
                .detach();
        }
    }
    pub(in crate::frontend) fn marks(&self) -> Value {
        self.marks_of(&self.page)
    }
    /// A page's row marks, under the interface's preference.
    pub(in crate::frontend) fn marks_of(&self, page: &str) -> Value {
        self.prefs.get(
            &format!(
                "rowMarks.{}",
                if page == "network" {
                    "connections"
                } else {
                    page
                }
            ),
            json!({}),
        )
    }
    pub(in crate::frontend) fn set_mark(
        &mut self,
        key: &str,
        field: &str,
        value: Value,
        cx: &mut Context<Self>,
    ) {
        let mut marks = self.marks();
        if !marks[key].is_object() {
            marks[key] = json!({});
        }
        marks[key][field] = value;
        self.prefs.set(
            &format!(
                "rowMarks.{}",
                if self.page == "network" {
                    "connections"
                } else {
                    &self.page
                }
            ),
            marks,
        );
        self.rebuild_rows(false, cx);
    }
    /// The running process `pid` in the latest snapshot.
    pub(super) fn process(&self, pid: u64) -> Option<&Value> {
        values(&self.snapshot["processes"])
            .iter()
            .find(|process| process["pid"].as_u64() == Some(pid))
    }
    /// Display names of the services running in each process, by PID.
    pub(in crate::frontend) fn hosted_services(&self) -> HashMap<u64, Vec<String>> {
        let mut hosted: HashMap<u64, Vec<String>> = HashMap::new();
        for service in self.data.get("services").map(values).unwrap_or_default() {
            if let Some(pid) = service["pid"].as_u64().filter(|pid| *pid != 0) {
                hosted
                    .entry(pid)
                    .or_default()
                    .push(text(service, "displayName").to_owned());
            }
        }
        hosted
    }
}
