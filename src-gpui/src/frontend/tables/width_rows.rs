//! The process list the automatic column widths measure (`WidthRows`), built
//! from the samples now and then.
use super::*;

/// How often, at most, the samples build the list the column widths
/// measure again (`WidthRows`). Rows on screen widen a column with each
/// sample in between, so only rows scrolled away, folded or left out by a
/// search wait for it.
pub(in crate::frontend) const WIDTH_ROWS_EVERY: Duration = Duration::from_secs(5);
/// The process list as the automatic column widths measure it: every group
/// open and nothing searched, so that searching or folding a group leaves
/// the widths as they are. Each row keeps only what the widths read of it
/// (`WidthRow`). Built again with another view, grouping, column choice
/// or language, and from the samples now and then; the rows shown can only
/// widen a column in between (`widths::AutoWidths::apply`).
pub(in crate::frontend) struct WidthRows {
    pub(super) rows: Arc<Vec<WidthRow>>,
    shape: WidthShape,
    built: Instant,
}
/// What the measured list depends on besides the sample: the view, the
/// category headings of the name order, grouping by user, the columns
/// chosen, the language and the byte units.
#[derive(PartialEq)]
struct WidthShape {
    view: String,
    sections: bool,
    users: model::UserGrouping,
    columns: Vec<String>,
    locale: argus_core::i18n::Locale,
    byte_unit: ByteUnit,
}
/// What the measured list is built from besides the sample, taken on the
/// UI thread for a build that may run off it.
pub(in crate::frontend) struct WidthSource {
    shape: WidthShape,
    sort: String,
    hosted: HashMap<u64, Vec<String>>,
    visibility: Option<visibility::Rules>,
    columns: columns::Preferences,
    bits: bool,
}
impl WidthSource {
    /// The rows of `snapshot`'s processes with every group open and nothing
    /// searched (`model::ProcessOptions::open`), as the widths read them.
    pub(in crate::frontend) fn build(self, snapshot: &Value) -> WidthRows {
        let none = HashSet::new();
        let users = model::UserGroups::grouping(values(&snapshot["sessions"]), self.shape.users);
        let rows = model::process_rows(
            values(&snapshot["processes"]),
            model::ProcessOptions {
                mode: &self.shape.view,
                query: "",
                sort: &self.sort,
                descending: false,
                expanded: &none,
                tree_expanded: &none,
                sections: &none,
                hosted: &self.hosted,
                visibility: self.visibility.as_ref(),
                users: users.as_ref(),
                user_folds: &none,
                open: true,
            },
        );
        let fields = process_fields(&self.columns, columns::Sources::of(snapshot));
        WidthRows {
            rows: Arc::new(
                rows.iter()
                    .map(|row| WidthRow::of(row, &fields, self.bits))
                    .collect(),
            ),
            shape: self.shape,
            built: Instant::now(),
        }
    }
}
impl Desktop {
    /// What the process list measured for the column widths depends on
    /// besides the sample.
    fn width_shape(&self, columns: &columns::Preferences) -> WidthShape {
        let mut keys = columns.selected.clone();
        keys.sort();
        WidthShape {
            view: self.view.clone(),
            sections: self.sort == "name",
            users: self.user_grouping(),
            columns: keys,
            locale: argus_core::i18n::locale(),
            byte_unit: ByteUnit::current(),
        }
    }
    /// What to build the process list measured for the column widths from,
    /// if it is to be built again: there is none, or the list takes another
    /// shape; with a new sample, also once it is `older` than that. Only
    /// while the process page sizes its columns automatically, and no build
    /// is under way.
    pub(in crate::frontend) fn width_source(&self, older: Option<Duration>) -> Option<WidthSource> {
        if self.page != "processes" || self.width_pending {
            return None;
        }
        let columns = columns::Preferences::load(&self.prefs);
        if !columns.auto_width {
            return None;
        }
        let shape = self.width_shape(&columns);
        let due = self.width_rows.as_ref().is_none_or(|built| {
            built.rows.is_empty()
                || built.shape != shape
                || older.is_some_and(|older| built.built.elapsed() >= older)
        });
        due.then(|| WidthSource {
            shape,
            sort: self.sort.clone(),
            hosted: self.hosted_services(),
            visibility: self.visibility_rules("processes"),
            columns,
            bits: self.prefs.get::<String>("netUnit", "bytes".into()) == "bits",
        })
    }
    /// Keeps `built` as the list the column widths measure, with the row marks.
    pub(in crate::frontend) fn keep_width_rows(&mut self, mut built: WidthRows) {
        if let Some(rows) = Arc::get_mut(&mut built.rows) {
            let marks = self.marks();
            for row in rows {
                if let Some(key) = &row.mark_key {
                    (row.note, row.pinned) = row_mark(&marks, key);
                }
            }
        }
        self.width_rows = Some(built);
    }
    /// Builds the list the column widths measure again if it is due
    /// (`width_source`): off the UI thread from the latest sample, the
    /// widths keeping the list they have until it is done. Test samples
    /// have no such source and build it here.
    pub(in crate::frontend) fn refresh_width_rows(
        &mut self,
        older: Option<Duration>,
        cx: &mut Context<Self>,
    ) {
        let Some(source) = self.width_source(older) else {
            return;
        };
        let latest = if self.options.fixture.is_none() {
            self.host.state::<crate::Latest>().0.read().unwrap().clone()
        } else {
            None
        };
        let Some(snapshot) = latest else {
            let built = source.build(&self.snapshot);
            self.keep_width_rows(built);
            return;
        };
        self.width_pending = true;
        let task = cx.background_executor().spawn(async move {
            let model = serde_json::to_value(snapshot.as_ref()).expect("snapshot model");
            source.build(&model)
        });
        cx.spawn(async move |this, cx| {
            let built = task.await;
            let _ = this.update(cx, |this, cx| {
                this.width_pending = false;
                this.keep_width_rows(built);
                if this.page == "processes" {
                    this.configure_table(cx);
                }
            });
        })
        .detach();
    }
}
