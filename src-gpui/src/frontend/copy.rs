//! Copying a list row: Ctrl+C its name, Ctrl+Shift+C and "Copy details"
//! every column shown, as "Column: value" lines.
//!
//! The details come from the row and the table's own cells
//! (`NativeTable::row_cells`), so they follow the columns, units and
//! language the table shows.
use super::model::{Row, text};
use super::theme::label;
use super::*;

/// The lists whose rows copy.
const PAGES: [&str; 4] = ["processes", "services", "startup", "network"];

/// Whether `row` of `page` is a copyable entry, not a heading.
fn copyable(page: &str, row: &Row) -> bool {
    PAGES.contains(&page) && row.section.is_none() && row.user.is_none()
}

/// What Ctrl+C copies of `row` on `page`, as the menus copy it: a process's
/// executable, a service's own name, otherwise the name column. Headings and
/// rows of other pages copy nothing.
pub(super) fn row_name(page: &str, row: &Row) -> Option<String> {
    if !copyable(page, row) {
        return None;
    }
    let raw_name = text(&row.raw, "name");
    let name = if matches!(page, "processes" | "services") && !raw_name.is_empty() {
        raw_name.to_owned()
    } else {
        row.title.clone()
    };
    (!name.is_empty()).then_some(name)
}

/// The headings a process's path and command line are listed under.
pub(super) struct ProcessHeadings {
    pub path: String,
    pub command_line: String,
}

/// The lines Ctrl+Shift+C copies of `row`: the non-empty `cells` the table
/// shows, then a process's path and command line when known and not shown
/// already. Headings and rows of other pages copy nothing.
pub(super) fn row_details(
    page: &str,
    row: &Row,
    cells: Vec<(String, String)>,
    headings: &ProcessHeadings,
) -> Vec<(String, String)> {
    if !copyable(page, row) {
        return Vec::new();
    }
    let mut lines: Vec<_> = cells
        .into_iter()
        .filter(|(_, value)| !value.trim().is_empty())
        .collect();
    if page == "processes" {
        for (heading, value) in [
            (&headings.path, row.raw["path"].as_str()),
            (&headings.command_line, row.column_text("commandLine")),
        ] {
            if let Some(value) = value.filter(|value| !value.trim().is_empty())
                && !lines.iter().any(|(shown, _)| shown == heading)
            {
                lines.push((heading.clone(), value.to_owned()));
            }
        }
    }
    lines
}

/// Lines as "Column: value", as Windows text editors expect them.
pub(super) fn details_text(lines: &[(String, String)]) -> String {
    lines
        .iter()
        .map(|(heading, value)| format!("{heading}: {value}"))
        .collect::<Vec<_>>()
        .join("\r\n")
}

impl Desktop {
    /// Copies the selected row of the shown list (`copy_row_of`).
    pub(in crate::frontend) fn copy_row(
        &mut self,
        full: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(row) = self
            .selected
            .get(&self.page)
            .and_then(|key| self.rows.iter().find(|row| &row.key == key))
            .cloned()
        else {
            return;
        };
        self.copy_row_of(&row, full, window, cx);
    }
    /// Copies `row` of the shown list: its name (`row_name`), or with `full`
    /// its details (`row_details`).
    pub(in crate::frontend) fn copy_row_of(
        &mut self,
        row: &Row,
        full: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !full {
            if let Some(name) = row_name(&self.page, row) {
                self.act(Action::Copy(name, "actions.copyWhat.name"), window, cx);
            }
            return;
        }
        let cells = self
            .table
            .as_ref()
            .map(|table| table.read(cx).delegate().row_cells(row))
            .unwrap_or_default();
        let headings = ProcessHeadings {
            path: label("processes.column.path.label"),
            command_line: label("processes.column.commandLine.label"),
        };
        let lines = row_details(&self.page, row, cells, &headings);
        if !lines.is_empty() {
            self.act(
                Action::Copy(details_text(&lines), "actions.copyWhat.details"),
                window,
                cx,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    // Named imports keep the glob-imported GPUI `test` macro out of scope.
    use super::{ProcessHeadings, Row, details_text, row_details, row_name};
    use serde_json::{Value, json};

    fn row(raw: Value, title: &str) -> Row {
        let mut row = Row::item(raw.clone(), "key".into(), title.into());
        row.members = vec![raw];
        row
    }
    fn headings() -> ProcessHeadings {
        ProcessHeadings {
            path: "PATH".into(),
            command_line: "CMD".into(),
        }
    }
    fn cell(heading: &str, value: &str) -> (String, String) {
        (heading.into(), value.into())
    }

    #[test]
    fn the_name_is_the_one_the_menus_copy() {
        let process = row(json!({"name":"firefox.exe","pid":42}), "Firefox");
        assert_eq!(
            row_name("processes", &process).as_deref(),
            Some("firefox.exe")
        );
        let service = row(
            json!({"name":"Dnscache","displayName":"DNS Client"}),
            "DNS Client",
        );
        assert_eq!(row_name("services", &service).as_deref(), Some("Dnscache"));
        // Startup entries and connections copy what their name column shows.
        let entry = row(json!({"name":"Updater","id":"run|x"}), "Updater (user)");
        assert_eq!(
            row_name("startup", &entry).as_deref(),
            Some("Updater (user)")
        );
        let connection = row(json!({"pid":42}), "Firefox");
        assert_eq!(row_name("network", &connection).as_deref(), Some("Firefox"));
        // A process without a name falls back to its title; nothing to copy is nothing.
        assert_eq!(
            row_name("processes", &row(json!({"pid":1}), "System")).as_deref(),
            Some("System")
        );
        assert_eq!(row_name("processes", &row(json!({}), "")), None);
        assert_eq!(row_name("performance", &process), None);
    }

    #[test]
    fn headings_copy_nothing() {
        let mut heading = row(json!({"name":"x.exe"}), "Apps");
        heading.section = Some("apps".into());
        assert_eq!(row_name("processes", &heading), None);
        assert!(
            row_details(
                "processes",
                &heading,
                vec![cell("Name", "Apps")],
                &headings()
            )
            .is_empty()
        );
    }

    #[test]
    fn details_add_a_process_path_and_command_line_once() {
        let process = row(
            json!({"name":"app.exe","path":"C:\\App\\app.exe","commandLine":"app.exe --x"}),
            "App",
        );
        let cells = vec![cell("Name", "App"), cell("PID", "42"), cell("Company", " ")];
        assert_eq!(
            row_details("processes", &process, cells, &headings()),
            [
                cell("Name", "App"),
                cell("PID", "42"),
                cell("PATH", "C:\\App\\app.exe"),
                cell("CMD", "app.exe --x"),
            ]
        );
        // Shown as columns already: not repeated.
        let cells = vec![cell("PATH", "C:\\App\\app.exe"), cell("CMD", "app.exe --x")];
        assert_eq!(
            row_details("processes", &process, cells.clone(), &headings()),
            cells
        );
        // Other lists copy their cells alone.
        assert_eq!(
            row_details("services", &process, vec![cell("Name", "App")], &headings()),
            [cell("Name", "App")]
        );
    }

    #[test]
    fn details_are_column_lines_for_windows_editors() {
        let lines = [cell("Name", "Firefox"), cell("PID", "4242")];
        assert_eq!(details_text(&lines), "Name: Firefox\r\nPID: 4242");
        assert_eq!(details_text(&[]), "");
    }
}
