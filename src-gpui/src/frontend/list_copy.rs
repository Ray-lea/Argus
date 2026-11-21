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

/// Whether a menu item copies text.
fn is_copy(item: &MenuItem) -> bool {
    matches!(item.action, Action::Copy(..) | Action::CopyAs(..))
}

/// `items` with their copy items and `more` in one "Copy" submenu (`title`),
/// as the process list's menu has it: where the first copy item was, else
/// at the end. A long menu then keeps its height in a short window.
pub(super) fn gather_copies(
    items: Vec<MenuItem>,
    more: Vec<MenuItem>,
    title: String,
) -> Vec<MenuItem> {
    let at = items.iter().position(is_copy);
    let (mut copies, mut rest): (Vec<_>, Vec<_>) = items.into_iter().partition(is_copy);
    for copy in &mut copies {
        // Submenu entries go without the icon the menu gives the submenu.
        copy.icon = None;
    }
    copies.extend(more);
    let at = at.unwrap_or(rest.len());
    rest.insert(
        at,
        MenuItem::new(title, Action::CloseOverlay)
            .icon("CopyIcon")
            .submenu(copies),
    );
    rest
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
    use super::{
        Action, MenuItem, cell_lines, copies_cells, copy_position, gather_copies, lines_text,
    };

    #[test]
    fn copies_gather_in_one_submenu_where_the_first_was() {
        let item = |label: &str, action| MenuItem::new(label, action);
        let copy = |label: &str| {
            item(label, Action::Copy("x".into(), "actions.copyWhat.name")).icon("CopyIcon")
        };
        let menu = vec![
            item("start", Action::CloseOverlay),
            item("open", Action::CloseOverlay),
            copy("service name"),
            MenuItem::separator(),
            item("delete", Action::CloseOverlay),
            copy("late copy"),
        ];
        let more = vec![
            item("display name", Action::CopyAs("y".into(), "name".into())),
            MenuItem::separator(),
            item("details", Action::CopyAs("z".into(), "details".into())),
        ];
        let menu = gather_copies(menu, more, "Copy".into());
        let labels: Vec<_> = menu.iter().map(|item| item.label.as_str()).collect();
        assert_eq!(labels, ["start", "open", "Copy", "", "delete"]);
        let submenu = &menu[2];
        assert_eq!(submenu.icon, Some("CopyIcon"));
        let children: Vec<_> = submenu
            .children
            .iter()
            .map(|item| item.label.as_str())
            .collect();
        assert_eq!(
            children,
            ["service name", "late copy", "display name", "", "details"]
        );
        assert!(submenu.children.iter().all(|child| child.icon.is_none()));
        // The cell's own copy then follows the submenu.
        assert_eq!(copy_position(&menu), 3);
        // A menu without copies takes the submenu at its end.
        let plain = gather_copies(
            vec![item("a", Action::CloseOverlay)],
            Vec::new(),
            "Copy".into(),
        );
        assert_eq!(plain.last().map(|item| item.label.as_str()), Some("Copy"));
    }

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
