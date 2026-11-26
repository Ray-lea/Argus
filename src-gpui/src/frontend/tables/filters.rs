//! The filters in a list column's header, as a spreadsheet's column filters.
use super::*;

/// A filter in a list column's header, as a spreadsheet's column filter: the
/// column, the interface's preference that keeps the choice, and each choice
/// with its label. A list's filters narrow it together.
pub(in crate::frontend) struct ColumnFilter {
    pub column: &'static str,
    pub preference: &'static str,
    /// The first choice, "all", leaves the list whole.
    pub choices: &'static [(&'static str, &'static str)],
}
impl ColumnFilter {
    pub(in crate::frontend) fn offers(&self, choice: &str) -> bool {
        self.choices.iter().any(|(offered, _)| *offered == choice)
    }
    /// The key its header button's menu hangs from.
    pub(in crate::frontend) fn anchor(&self) -> String {
        format!("{COLUMN_FILTER}.{}", self.column)
    }
}
const SERVICE_FILTERS: &[ColumnFilter] = &[ColumnFilter {
    column: "state",
    preference: "services.filter",
    choices: &[
        ("all", "services.filter.all"),
        ("running", "services.filter.running"),
        ("stopped", "services.filter.stopped"),
    ],
}];
/// Protocols as the table names them: "tcp6" is TCP over IPv6.
const CONNECTION_FILTERS: &[ColumnFilter] = &[
    ColumnFilter {
        column: "protocol",
        preference: "connections.protocolFilter",
        choices: &[
            ("all", "connections.filter.all.label"),
            ("tcp", "connections.protocolFilter.tcp"),
            ("tcp6", "connections.protocolFilter.tcp6"),
            ("udp", "connections.protocolFilter.udp"),
            ("udp6", "connections.protocolFilter.udp6"),
        ],
    },
    ColumnFilter {
        column: "state",
        preference: "connections.filter",
        choices: &[
            ("all", "connections.filter.all.label"),
            ("listen", "connections.filter.listen.label"),
            ("established", "connections.filter.established.label"),
        ],
    },
];
/// The column filters of `page`'s list.
pub(in crate::frontend) fn column_filters(page: &str) -> &'static [ColumnFilter] {
    match page {
        "services" => SERVICE_FILTERS,
        "network" => CONNECTION_FILTERS,
        _ => &[],
    }
}
/// The filter in the header of `page`'s column `column`, if it has one.
pub(in crate::frontend) fn column_filter(
    page: &str,
    column: &str,
) -> Option<&'static ColumnFilter> {
    column_filters(page)
        .iter()
        .find(|filter| filter.column == column)
}
fn all_filters() -> impl Iterator<Item = &'static ColumnFilter> {
    SERVICE_FILTERS.iter().chain(CONNECTION_FILTERS)
}
/// The column filter's button, and the key its menus hang from.
pub(super) const FILTER_BUTTON: f32 = 22.;
pub(in crate::frontend) const COLUMN_FILTER: &str = "column-filter";
/// The saved choices of the column filters, by preference; an unknown value
/// is "all".
pub(in crate::frontend) fn saved_filters(prefs: &prefs::Prefs) -> HashMap<&'static str, String> {
    all_filters()
        .filter_map(|filter| {
            let saved = prefs.get::<String>(filter.preference, "all".into());
            (saved != "all" && filter.offers(&saved)).then_some((filter.preference, saved))
        })
        .collect()
}
impl Desktop {
    /// Narrows a list by the column filter kept under `preference` and keeps
    /// the choice for the next start; an unknown choice is "all".
    pub(in crate::frontend) fn set_filter(
        &mut self,
        preference: &str,
        choice: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(filter) = all_filters().find(|filter| filter.preference == preference) else {
            return;
        };
        let choice = if filter.offers(choice) { choice } else { "all" };
        self.filters.insert(filter.preference, choice.into());
        self.prefs.set(filter.preference, choice);
        self.rebuild_rows(false, cx);
    }
    /// Sets every column filter of `page`'s list back to "all".
    pub(in crate::frontend) fn clear_filters(&mut self, page: &str, cx: &mut Context<Self>) {
        for filter in column_filters(page) {
            self.filters.insert(filter.preference, "all".into());
            self.prefs.set(filter.preference, "all");
        }
        self.rebuild_rows(false, cx);
    }
}

#[cfg(test)]
mod tests {
    // Named imports keep the glob-imported GPUI `test` macro out of scope.
    use super::{prefs, saved_filters};

    #[test]
    fn filters_use_persistent_preference_keys() {
        let file =
            std::env::temp_dir().join(format!("argus-native-filters-{}.json", std::process::id()));
        std::fs::write(
            &file,
            // "udp" was a status choice before the protocol column had its own filter.
            br#"{"values":{"services.filter":"stopped","connections.filter":"udp","connections.protocolFilter":"tcp6"}}"#,
        )
        .unwrap();
        let prefs = prefs::Prefs::load(file.clone());
        std::fs::remove_file(&file).unwrap();
        let filters = saved_filters(&prefs);
        assert_eq!(
            filters.get("services.filter").map(String::as_str),
            Some("stopped")
        );
        assert_eq!(
            filters
                .get("connections.protocolFilter")
                .map(String::as_str),
            Some("tcp6")
        );
        assert!(!filters.contains_key("connections.filter"));
    }
}
