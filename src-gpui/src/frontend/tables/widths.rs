//! Process column widths settled ahead of the values. A numeric column starts
//! as wide as the widest value its format shows in practice (`envelope`), a
//! text column as its widest value in any row, not only the visible ones, and
//! a column only ever widens, in steps. Digits measure alike, so a value
//! changing keeps its width: the columns stay put while the readings move.
use super::*;
use std::ops::Range;
use std::time::Duration;

pub(super) type CellValues = HashMap<(usize, &'static str), String>;

/// How often every row is measured, besides when rows come or go; the
/// visible ones are measured with each sample.
const FULL_PASS: Duration = Duration::from_secs(2);
/// Widths grow in steps this wide: a slightly longer value does not move
/// every column after it.
const STEP: f32 = 8.;

#[derive(Default)]
pub(super) struct AutoWidths {
    pub widths: HashMap<&'static str, f32>,
    measured: HashMap<(u8, String), (f32, u64)>,
    epoch: u64,
    /// When every row was last measured, and how many rows there were.
    full: Option<(Instant, usize)>,
}

pub(super) struct Contents<'a> {
    pub rows: &'a [Row],
    pub range: Range<usize>,
    pub totals: &'a HashMap<String, String>,
    pub values: &'a mut CellValues,
    pub bits: bool,
}

/// The widest values a numeric format shows in practice: a column is this
/// wide from the start instead of creeping wider as readings grow. A rarer,
/// wider value still widens it.
fn envelope(field: &Field, bits: bool) -> &'static [&'static str] {
    match field.kind {
        CellKind::Percent => &["100%", "8.8%"],
        CellKind::Memory if field.key.starts_with("io") && field.key.ends_with("Total") => {
            &["888.8 GB", "8888 KB"]
        }
        CellKind::Memory => &["88.8 GB", "8.88 GB", "888.8 MB"],
        CellKind::AddressSpace => &["888.8 TB", "888.8 GB"],
        CellKind::NetworkRate if bits => &["888.8 Mbps", "888.8 Kbps"],
        CellKind::Rate | CellKind::NetworkRate => &["8888 KB/s", "888.8 MB/s", "8.88 GB/s"],
        _ => &[],
    }
}

/// The widest totals above such a column, in the header's larger figures.
fn total_envelope(field: &Field, bits: bool) -> &'static [&'static str] {
    match field.key {
        "cpu" | "mem" | "gpu" => &["100%"],
        _ if !envelope(field, bits).is_empty() && field.kind != CellKind::Percent => {
            envelope(field, bits)
        }
        _ => &[],
    }
}

/// Whether a column shows figures, whose digits all measure alike.
fn numeric(kind: CellKind) -> bool {
    matches!(
        kind,
        CellKind::Count
            | CellKind::Identifier
            | CellKind::Pid
            | CellKind::Duration
            | CellKind::Timestamp
            | CellKind::Percent
            | CellKind::Memory
            | CellKind::AddressSpace
            | CellKind::Rate
            | CellKind::NetworkRate
    )
}

/// Figures with every digit drawn as an 8: the interface's digits are
/// tabular, so this measures the same and one measurement serves every
/// value with as many digits.
fn figures(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_ascii_digit() { '8' } else { c })
        .collect()
}

/// A width rounded up to the next step.
fn stepped(width: f32) -> f32 {
    (width / STEP).ceil() * STEP
}

impl AutoWidths {
    /// Measures every column anew (another text size or language).
    pub fn reset(&mut self) {
        self.widths.clear();
        self.measured.clear();
        self.full = None;
    }

    fn measure(
        &mut self,
        text: &str,
        role: u8,
        system: &gpui_kit::gpui::WindowTextSystem,
        cx: &App,
    ) -> f32 {
        if text.is_empty() {
            return 0.;
        }
        if text.len() > 1024 {
            return 2400.;
        }
        let text = text.replace(['\r', '\n', '\t'], " ");
        let key = (role, text.clone());
        if let Some((width, epoch)) = self.measured.get_mut(&key) {
            *epoch = self.epoch;
            return *width;
        }
        let size = theme::font(match role {
            0 => 12.,
            1 => 15.,
            _ => 13.,
        });
        let mut run = theme::text_run(&text, theme::LABEL_WEIGHT, cx);
        if role == 1 {
            run.font.family = theme::display_font(cx);
        }
        let width = f32::from(system.layout_line(&text, size, &[run], None).width).ceil();
        if self.measured.len() >= 4096 {
            let epoch = self.epoch;
            self.measured
                .retain(|_, (_, seen)| epoch.saturating_sub(*seen) <= 2);
            if self.measured.len() >= 4096 {
                self.measured.clear();
            }
        }
        self.measured.insert(key, (width, self.epoch));
        width
    }

    pub fn apply(&mut self, fields: &mut [Field], contents: Contents<'_>, cx: &App) {
        self.epoch = self.epoch.wrapping_add(1);
        let system = gpui_kit::gpui::WindowTextSystem::new(cx.text_system().clone());
        let now = Instant::now();
        let count = contents.rows.len();
        let full = self
            .full
            .is_none_or(|(at, rows)| rows != count || now.duration_since(at) >= FULL_PASS);
        if full {
            self.full = Some((now, count));
        }
        for field in fields {
            let numeric = numeric(field.kind);
            let shown = |text: &str| {
                if numeric {
                    figures(text)
                } else {
                    text.to_owned()
                }
            };
            let header = self.measure(&field.label, 0, &system, cx) + CELL_PADDING * 2. + 20.;
            let mut total = self.measure(
                &shown(
                    contents
                        .totals
                        .get(field.key)
                        .map(String::as_str)
                        .unwrap_or(""),
                ),
                1,
                &system,
                cx,
            );
            for text in total_envelope(field, contents.bits) {
                total = total.max(self.measure(text, 1, &system, cx));
            }
            let total = total + CELL_PADDING * 2. + 6.;
            let mut required = header.max(total);
            let bounds = envelope(field, contents.bits);
            for text in bounds {
                required = required.max(self.measure(text, 2, &system, cx) + CELL_PADDING * 2.);
            }
            // A column with an envelope checks the visible rows for rarer,
            // wider values; the others measure every row now and then, so
            // rows scrolled to later fit already.
            let range = if full && bounds.is_empty() {
                0..count
            } else {
                contents.range.clone()
            };
            for index in range {
                let Some(row) = contents.rows.get(index) else {
                    continue;
                };
                let text = contents
                    .values
                    .entry((index, field.key))
                    .or_insert_with(|| display_cell(row, field, contents.bits));
                let text = shown(text);
                let mut width = self.measure(&text, 2, &system, cx) + CELL_PADDING * 2.;
                if field.key == "name" {
                    width += 48. + row.depth as f32 * 16.;
                    if !row.detail.is_empty() {
                        width += self.measure(&row.detail, 0, &system, cx) + 10.;
                    }
                }
                required = required.max(width);
            }
            let (minimum, limit): (f32, f32) = match field.key {
                "name" => (180., 400.),
                "path" => (120., 600.),
                "description" => (100., 480.),
                "company" | "gpuEngine" => (100., 360.),
                _ => (64., 400.),
            };
            let required = stepped(required.clamp(minimum, limit.max(header).max(total)));
            let width = self.widths.entry(field.key).or_insert(required);
            *width = width.max(required);
            field.track = if field.key == "name" {
                Track::Flex(*width, 1.)
            } else {
                Track::Fixed(*width)
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{figures, stepped};

    #[test]
    fn figures_measure_alike_and_widths_grow_in_steps() {
        assert_eq!(figures("1,195.4 MB/s"), "8,888.8 MB/s");
        assert_eq!(figures("PID 4"), "PID 8");
        assert_eq!(stepped(64.), 64.);
        assert_eq!(stepped(64.5), 72.);
        assert_eq!(stepped(71.9), 72.);
    }
}
