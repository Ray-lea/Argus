//! CSV snapshots of the rows and columns currently presented by a table.
use super::{CellKind, Desktop, Field, NativeTable, Row, display_cell, label, widgets::ToastKind};
use gpui_kit::{App, Context, Window};
use std::{
    io::{self, BufWriter, Write},
    path::Path,
    sync::Arc,
};
use time::OffsetDateTime;

struct CsvTable {
    fields: Vec<Field>,
    rows: Arc<Vec<Row>>,
    bits: bool,
}

impl CsvTable {
    fn write_to(&self, out: &mut impl Write) -> io::Result<()> {
        out.write_all(b"\xef\xbb\xbf")?;
        write_record(out, self.fields.iter().map(|field| &field.label))?;
        for row in self.rows.iter() {
            write_record(
                out,
                self.fields.iter().map(|field| {
                    let mut value = display_cell(row, field, self.bits);
                    // Names, descriptions and command lines are external text. Keep a
                    // spreadsheet from executing a formula when it opens the export.
                    if matches!(
                        field.kind,
                        CellKind::Name
                            | CellKind::Dim
                            | CellKind::Mono
                            | CellKind::MonoDim
                            | CellKind::ProcessText
                    ) && (value.starts_with(['\t', '\r', '\n'])
                        || value.trim_start().starts_with(['=', '+', '-', '@']))
                    {
                        value.insert(0, '\'');
                    }
                    value
                }),
            )?;
        }
        Ok(())
    }

    fn save(&self, path: &Path) -> io::Result<()> {
        let mut file = tempfile::NamedTempFile::new_in(path.parent().unwrap_or(Path::new(".")))?;
        {
            let mut out = BufWriter::new(file.as_file_mut());
            self.write_to(&mut out)?;
            out.flush()?;
        }
        file.persist(path)?;
        Ok(())
    }
}

fn write_record(
    out: &mut impl Write,
    cells: impl Iterator<Item = impl AsRef<str>>,
) -> io::Result<()> {
    for (index, cell) in cells.enumerate() {
        if index > 0 {
            out.write_all(b",")?;
        }
        let cell = cell.as_ref();
        if cell.contains([',', '"', '\r', '\n']) {
            write!(out, "\"{}\"", cell.replace('"', "\"\""))?;
        } else {
            out.write_all(cell.as_bytes())?;
        }
    }
    out.write_all(b"\r\n")
}

fn suggested_name(page: &str, view: &str, rows: usize, now: OffsetDateTime) -> String {
    let mut name = label(&format!("shell.page.{page}"));
    if page == "processes" {
        name.push('-');
        name.push_str(&label(&format!("shell.viewMode.{view}.label")));
    }
    let count = argus_core::tr!("shell.more.csvRows", count = rows);
    format!(
        "Argus-{name}-{}_{:02}-{:02}-{:02}-{count}.csv",
        now.date(),
        now.hour(),
        now.minute(),
        now.second()
    )
}

impl Desktop {
    pub(in crate::frontend) fn exportable_table<'a>(
        &'a self,
        cx: &'a App,
    ) -> Option<&'a NativeTable> {
        let table = self.table.as_ref()?.read(cx).delegate();
        (table.page == self.page && !table.fields.is_empty()).then_some(table)
    }

    pub(in crate::frontend) fn export_table_csv(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(table) = self.exportable_table(cx) else {
            return;
        };
        let export = CsvTable {
            fields: table.fields.clone(),
            rows: table.rows.clone(),
            bits: table.bits,
        };
        let now = OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc());
        let name = suggested_name(&table.page, &table.scope.0, export.rows.len(), now);
        if self.options.fixture.is_some() {
            let mut bytes = Vec::new();
            export.write_to(&mut bytes).expect("write CSV to memory");
            let content = String::from_utf8(bytes).expect("CSV is UTF-8");
            self.data.insert(
                "tableCsv".into(),
                serde_json::json!({ "name": name, "content": content }),
            );
            return;
        }
        let picker = rfd::AsyncFileDialog::new()
            .set_parent(window)
            .add_filter(label("shell.more.csvFile"), &["csv"])
            .set_file_name(name)
            .save_file();
        cx.spawn(async move |this, cx| {
            let Some(file) = picker.await else { return };
            let result = cx
                .background_executor()
                .spawn(async move { export.save(file.path()) })
                .await;
            let _ = this.update(cx, |this, cx| {
                let (kind, key) = match result {
                    Ok(()) => (ToastKind::Success, "shell.more.csvSaved"),
                    Err(_) => (ToastKind::Error, "shell.more.csvFailed"),
                };
                this.notify_toast(kind, label(key));
                cx.notify();
            });
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::{CellKind, CsvTable, Field, Row, suggested_name};
    use crate::frontend::tables::Track;
    use serde_json::json;
    use std::sync::Arc;

    #[test]
    fn filenames_use_local_time_and_distinguish_pages_views_and_row_counts() {
        crate::catalog::register();
        let now = time::OffsetDateTime::from_unix_timestamp(0)
            .unwrap()
            .to_offset(time::UtcOffset::from_hms(-8, 0, 0).unwrap());
        let name = |page, view, count| suggested_name(page, view, count, now);
        let list = name("processes", "list", 12);
        assert!(list.contains("1969-12-31_16-00-00"));
        assert!(list.ends_with(".csv"));
        assert!(!list.contains(['<', '>', ':', '"', '/', '\\', '|', '?', '*']));
        assert_ne!(list, name("processes", "tree", 12));
        assert_ne!(list, name("processes", "list", 0));
        assert_ne!(list, name("startup", "list", 12));
        assert_eq!(name("network", "apps", 12), name("network", "tree", 12));
    }

    #[test]
    fn export_replaces_the_file_with_the_current_columns_and_rows() {
        let field = |key, kind| Field {
            key,
            kind,
            label: key.to_owned(),
            track: Track::Fixed(80.),
            end: false,
        };
        let mut table = CsvTable {
            fields: vec![
                field("command", CellKind::MonoDim),
                field("name", CellKind::Name),
            ],
            rows: Arc::new(vec![
                Row::item(json!({"command":"a,\"b\"\r\nc"}), "1".into(), "中文".into()),
                Row::item(json!({"command":"\t=1+1"}), "2".into(), " @test".into()),
            ]),
            bits: false,
        };
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("table.csv");
        table.save(&path).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "\u{feff}command,name\r\n\"a,\"\"b\"\"\r\nc\",中文\r\n'\t=1+1,' @test\r\n"
        );
        table.rows = Arc::new(Vec::new());
        table.save(&path).unwrap();
        assert_eq!(
            std::fs::read_to_string(path).unwrap(),
            "\u{feff}command,name\r\n"
        );
    }
}
