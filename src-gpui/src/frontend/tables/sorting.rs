//! The order a table opens in, and the saved orders it keeps.
use super::*;

/// A table's first column, which orders by name.
pub(in crate::frontend) fn first_column(page: &str) -> &'static str {
    fields(page, columns::Sources::ALL, f32::MAX)
        .first()
        .map(|field| field.key)
        .unwrap_or("name")
}
/// The order a table opens in.
pub(in crate::frontend) fn initial_sort(page: &str) -> (String, bool) {
    match page {
        "services" => ("displayName".into(), false),
        "network" => ("localPort".into(), false),
        "history" => ("cpu".into(), true),
        _ => ("name".into(), false),
    }
}
/// The interface's table behind each list and its sortable columns, as (field
/// here, column id there). `tableSort.<table>` keeps the saved preferences's
/// `{"id": column, "desc": bool}`, so both interfaces read the
/// same saved order. Columns the table does not sort by are left out.
type SortColumns = (&'static str, &'static [(&'static str, &'static str)]);
fn sort_columns(page: &str) -> Option<SortColumns> {
    Some(match page {
        "services" => (
            "services",
            &[
                ("displayName", "display"),
                ("name", "name"),
                ("pid", "pid"),
                ("state", "state"),
                ("startType", "start"),
            ],
        ),
        "startup" => (
            "startup",
            &[
                ("name", "name"),
                ("publisher", "publisher"),
                ("enabled", "state"),
                ("location", "location"),
            ],
        ),
        "network" => (
            "connections",
            &[
                ("name", "process"),
                ("pid", "pid"),
                ("protocol", "proto"),
                ("localAddress", "local"),
                ("localPort", "lport"),
                ("remoteAddress", "remote"),
                ("remotePort", "rport"),
                ("state", "state"),
            ],
        ),
        "history" => (
            "history",
            &[
                ("name", "name"),
                ("pid", "pid"),
                ("cpu", "cpu"),
                ("mem", "mem"),
                ("io", "io"),
                ("net", "net"),
                ("gpu", "gpu"),
            ],
        ),
        _ => return None,
    })
}
/// Whether `page`'s table sorts by the column `key`; the table disables the
/// headers of the others (a column without `sortValue`).
pub(in crate::frontend) fn sortable(page: &str, key: &str) -> bool {
    if page == "processes" {
        return columns::definition(key).is_some();
    }
    sort_columns(page).is_none_or(|(_, columns)| columns.iter().any(|(field, _)| *field == key))
}
/// A saved `tableSort.<table>` value as a sort of `page`, if it names one of
/// the table's sortable columns (the interface's validator).
fn restore_sort(page: &str, saved: &Value) -> Option<(String, bool)> {
    let (_, columns) = sort_columns(page)?;
    let id = saved["id"].as_str()?;
    let descending = saved["desc"].as_bool()?;
    columns
        .iter()
        .find(|(_, column)| *column == id)
        .map(|(field, _)| ((*field).to_owned(), descending))
}
/// The order a list opens in after a restart: the saved one, else the default.
pub(in crate::frontend) fn saved_sort(page: &str, prefs: &prefs::Prefs) -> (String, bool) {
    sort_columns(page)
        .and_then(|(table, _)| {
            restore_sort(page, &prefs.get(&format!("tableSort.{table}"), Value::Null))
        })
        .unwrap_or_else(|| initial_sort(page))
}
/// The preference that keeps a sort of `page`, and its value.
pub(in crate::frontend) fn sort_preference(
    page: &str,
    key: &str,
    descending: bool,
) -> Option<(String, Value)> {
    if page == "processes" {
        return Some(("sort".into(), json!({"key":key,"desc":descending})));
    }
    let (table, columns) = sort_columns(page)?;
    let (_, column) = columns.iter().find(|(field, _)| *field == key)?;
    Some((
        format!("tableSort.{table}"),
        json!({"id":column,"desc":descending}),
    ))
}
/// Text columns start ascending and figures descending, the useful first view.
pub(in crate::frontend) fn descending_first(page: &str, key: &str) -> bool {
    match page {
        "processes" => columns::definition(key).is_some_and(|column| {
            !matches!(
                column.format,
                columns::Format::Name
                    | columns::Format::Text
                    | columns::Format::Identifier
                    | columns::Format::Timestamp
            )
        }),
        "services" => key == "pid",
        "startup" => key == "enabled",
        "network" => matches!(key, "pid" | "localPort" | "remotePort"),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    // Named imports keep the glob-imported GPUI `test` macro out of scope.
    use super::{Value, json, restore_sort, sort_columns, sort_preference, sortable};

    #[test]
    fn saved_orders_keep_persistent_column_ids() {
        // Values a preferences.json from the saved preferences holds.
        assert_eq!(
            restore_sort("services", &json!({"id":"start","desc":true})),
            Some(("startType".into(), true))
        );
        assert_eq!(
            restore_sort("network", &json!({"id":"lport","desc":false})),
            Some(("localPort".into(), false))
        );
        assert_eq!(
            restore_sort("startup", &json!({"id":"state","desc":true})),
            Some(("enabled".into(), true))
        );
        for page in ["services", "startup", "network", "history"] {
            let (table, columns) = sort_columns(page).unwrap();
            for (field, _) in columns {
                let (key, value) = sort_preference(page, field, true).unwrap();
                assert_eq!(key, format!("tableSort.{table}"));
                assert_eq!(restore_sort(page, &value), Some(((*field).into(), true)));
            }
        }
        assert_eq!(
            sort_preference("processes", "cpu", true),
            Some(("sort".into(), json!({"key":"cpu","desc":true})))
        );
    }

    #[test]
    fn only_supported_columns_sort() {
        assert!(!sortable("services", "description"));
        assert!(!sortable("startup", "command"));
        assert!(sortable("services", "startType") && sortable("processes", "threads"));
        assert_eq!(sort_preference("services", "description", false), None);
        // The interface's validator: unsortable or unknown columns and malformed
        // values give way to the default order.
        for saved in [
            json!({"id":"desc","desc":false}),
            json!({"id":"displayName","desc":false}),
            json!({"id":"display"}),
            json!({"id":"display","desc":"true"}),
            Value::Null,
        ] {
            assert_eq!(restore_sort("services", &saved), None);
        }
    }
}
