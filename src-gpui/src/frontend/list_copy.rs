//! Copying from the service, startup and connection lists as the process
//! list does: Ctrl+C the selection's name, Ctrl+Shift+C its details, and a
//! right press on a cell offers that column's text.
//!
//! These lists have no details pane: their details are the columns the
//! table shows, read through the table so they keep its units and language.
use super::*;

/// The lists whose rows copy their cells. The process list copies its own
/// details (`Desktop::details_text`) but shares the column copy.
pub(super) const LISTS: [&str; 4] = ["processes", "services", "startup", "network"];

/// Whether `page` copies its details from the table's cells.
pub(super) fn copies_cells(page: &str) -> bool {
    LISTS.contains(&page) && page != "processes"
}

/// The cells worth copying: text a reader can use, without empty cells or
/// the placeholder for a missing reading.
pub(super) fn cell_lines(cells: Vec<(String, String)>, missing: &str) -> Vec<(String, String)> {
    cells
        .into_iter()
        .filter(|(_, value)| !value.trim().is_empty() && value != missing)
        .collect()
}

/// `lines` as text, each through `field` ("Name: value"), one per line as
/// the process details are.
pub(super) fn lines_text(
    lines: &[(String, String)],
    field: impl Fn(&str, &str) -> String,
) -> String {
    lines
        .iter()
        .map(|(name, value)| field(name, value))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Where a row menu takes more copy items: after its last copy item (a copy
/// action, or the process list's copy submenu), else at its end.
pub(super) fn copy_position(items: &[MenuItem]) -> usize {
    items
        .iter()
        .rposition(|item| {
            matches!(item.action, Action::Copy(..) | Action::CopyAs(..))
                || (!item.children.is_empty() && item.icon == Some("CopyIcon"))
        })
        .map_or(items.len(), |at| at + 1)
}

impl Desktop {
    /// The details of `row` on a list that copies its cells: every column
    /// the table shows. None when there is nothing to copy.
    pub(in crate::frontend) fn cells_text(&self, row: &Row, cx: &App) -> Option<String> {
        if !copies_cells(&self.page) || row.section.is_some() {
            return None;
        }
        let lines = cell_lines(self.row_cells(row, cx), &label("format.missing"));
        (!lines.is_empty()).then(|| {
            lines_text(&lines, |name, value| {
                tr!("details.copy.field", name = name, value = value)
            })
        })
    }
}

#[cfg(test)]
mod tests {
    // Named imports keep the glob-imported GPUI `test` macro out of scope.
    use super::{Action, MenuItem, cell_lines, copies_cells, copy_position, lines_text};

    fn cell(name: &str, value: &str) -> (String, String) {
        (name.into(), value.into())
    }

    #[test]
    fn only_the_other_lists_copy_their_cells() {
        for page in ["services", "startup", "network"] {
            assert!(copies_cells(page));
        }
        for page in ["processes", "performance", "history"] {
            assert!(!copies_cells(page));
        }
    }

    #[test]
    fn details_leave_out_empty_and_missing_cells() {
        let cells = vec![
            cell("Name", "Dnscache"),
            cell("PID", ""),
            cell("Description", "  "),
            cell("Publisher", "—"),
            cell("Status", "Running"),
        ];
        let lines = cell_lines(cells, "—");
        assert_eq!(lines, [cell("Name", "Dnscache"), cell("Status", "Running")]);
        assert_eq!(
            lines_text(&lines, |name, value| format!("{name}={value}")),
            "Name=Dnscache\nStatus=Running"
        );
        assert_eq!(lines_text(&[], |name, _| name.into()), "");
    }

    #[test]
    fn copy_items_join_the_menu_after_its_last_copy() {
        let item = |action| MenuItem::new("item", action);
        let copy = || item(Action::Copy("x".into(), "actions.copyWhat.name"));
        let menu = [
            item(Action::CloseOverlay),
            copy(),
            item(Action::CloseOverlay),
            copy(),
            MenuItem::separator(),
            item(Action::CloseOverlay),
        ];
        assert_eq!(copy_position(&menu), 4);
        let submenu = [
            item(Action::CloseOverlay),
            item(Action::CloseOverlay)
                .icon("CopyIcon")
                .submenu(vec![copy()]),
            MenuItem::separator(),
        ];
        assert_eq!(copy_position(&submenu), 2);
        let none = [item(Action::CloseOverlay), MenuItem::separator()];
        assert_eq!(copy_position(&none), 2);
    }
}
