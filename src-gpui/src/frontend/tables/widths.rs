//! Process column widths settled ahead of the values. A numeric column starts
//! as wide as the widest value its format shows in practice (`envelope`) and
//! only ever widens, in steps: digits measure alike, so a value changing keeps
//! its width and the columns stay put while the readings move.
//!
//! A text column (the name, a path, a description) is measured over every
//! row, not only the visible ones. It is as wide as most rows need
//! (`Natural::typical`), the few much longer values cut short with an
//! ellipsis, and widens toward what every row needs while the table has room
//! to spare (`allocate`). No column stretches to fill the table: what is left
//! stays empty on the right.
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
/// How long a column takes to reach a new width.
pub(super) const RESIZE: Duration = Duration::from_millis(240);

/// The text sizes measured: header labels, header totals, cell text,
/// group headings and the small counts beside a name.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Role {
    Label,
    Total,
    Cell,
    Heading,
    Badge,
}

/// A column's widths before the table's room is shared out.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Natural {
    /// What most rows need: a rare, much longer value is cut short.
    pub typical: f32,
    /// What every row needs, up to the column's limit.
    pub full: f32,
}

impl Natural {
    /// A column that takes one width whatever the room (a figure, or a
    /// width set by hand).
    pub fn fixed(width: f32) -> Self {
        Self {
            typical: width,
            full: width,
        }
    }
}

#[derive(Default)]
pub(super) struct AutoWidths {
    /// Each measured column's widths, by key.
    pub natural: HashMap<&'static str, Natural>,
    measured: HashMap<(Role, String), (f32, u64)>,
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
        // A process moving gigabytes a second is rare enough to widen its
        // column when it happens.
        CellKind::Rate | CellKind::NetworkRate => &["8888 KB/s", "888.8 MB/s"],
        _ => &[],
    }
}

/// The widest totals above such a column, in the header's larger figures:
/// a busy machine's totals reach these within moments (the disks' I/O runs
/// into hundreds of megabytes a second), so the column is that wide from the
/// start rather than widening, and cutting the total short, as they climb.
fn total_envelope(field: &Field, bits: bool) -> &'static [&'static str] {
    match field.key {
        "cpu" | "mem" | "gpu" => &["100%"],
        _ if field.kind != CellKind::Percent => envelope(field, bits),
        _ => &[],
    }
}

/// How far from its right edge a header's content stops: the table keeps
/// room there for a sort icon of its own (its small cells' padding). A
/// process header's own padding is that much narrower on the right.
pub(super) const HEADER_INSET: f32 = 6.;
/// The sort arrow beside a header's label, and the gap before it.
const SORT_ARROW: f32 = 14.;

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

/// Whether a column shows free text of any length, which may be cut short.
fn text(kind: CellKind) -> bool {
    matches!(kind, CellKind::Name | CellKind::ProcessText)
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

/// The narrowest and widest a column is made, whatever its contents.
fn limits(key: &str) -> (f32, f32) {
    match key {
        "name" => (180., 520.),
        "path" | "commandLine" => (120., 600.),
        "description" => (100., 480.),
        "company" | "gpuEngine" => (100., 360.),
        "imageName" => (100., 320.),
        _ => (64., 400.),
    }
}

/// What most of `widths` need: every value but the outliers, which lie
/// more than one and a half interquartile ranges above the upper quartile
/// (Tukey's fence). At least nineteen values in twenty always fit, so a
/// column of many long values is not cut down to the short ones.
fn typical(mut widths: Vec<f32>) -> f32 {
    if widths.is_empty() {
        return 0.;
    }
    widths.sort_by(f32::total_cmp);
    let at = |quantile: f32| widths[((widths.len() - 1) as f32 * quantile).round() as usize];
    let (lower, upper) = (at(0.25), at(0.75));
    let fence = (upper + 1.5 * (upper - lower)).max(at(0.95));
    widths
        .iter()
        .copied()
        .filter(|width| *width <= fence)
        .fold(0., f32::max)
}

/// A measured width brought to `current`: it widens at once, and narrows
/// only by more than a step, so a value coming and going at the edge of a
/// step does not move the columns back and forth.
fn settled(current: Option<f32>, measured: f32) -> f32 {
    match current {
        Some(current) if measured <= current && measured > current - STEP * 2. => current,
        _ => measured,
    }
}

/// The widths of the columns for `room`: each column takes what most of its
/// rows need, and those with longer values share what room is left, each in
/// proportion to what it lacks, up to what every row needs. Widths are
/// whole pixels; their sum may fall short of the room, never beyond what
/// the columns need.
pub(super) fn allocate(columns: &[Natural], room: f32) -> Vec<f32> {
    let base: f32 = columns.iter().map(|column| column.typical).sum();
    let wanted: f32 = columns
        .iter()
        .map(|column| (column.full - column.typical).max(0.))
        .sum();
    let share = if wanted > 0. {
        ((room - base).max(0.) / wanted).min(1.)
    } else {
        0.
    };
    columns
        .iter()
        .map(|column| {
            (column.typical + (column.full - column.typical).max(0.) * share)
                .floor()
                .max(column.typical)
        })
        .collect()
}

impl AutoWidths {
    /// Measures every column anew (another text size or language).
    pub fn reset(&mut self) {
        self.natural.clear();
        self.measured.clear();
        self.full = None;
    }

    fn measure(
        &mut self,
        text: &str,
        role: Role,
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
            Role::Label => 12.,
            Role::Total => 15.,
            Role::Cell => 13.,
            Role::Heading => 14.,
            Role::Badge => 11.,
        });
        let mut run = theme::text_run(&text, theme::LABEL_WEIGHT, cx);
        if role == Role::Total {
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

    /// What the name cell of `row` takes, laid out as `NativeTable::name_cell`
    /// draws it: the indent, the fold button or its space, the icon, the name
    /// and whatever follows it.
    fn name_width(
        &mut self,
        row: &Row,
        system: &gpui_kit::gpui::WindowTextSystem,
        cx: &App,
    ) -> f32 {
        const GAP: f32 = 8.;
        const FOLD: f32 = 18.;
        let edges = CELL_PADDING * 2. + 6.;
        if row.user.is_some() {
            return edges
                + FOLD
                + GAP
                + 16.
                + GAP
                + self.measure(&row.title, Role::Heading, system, cx)
                + GAP
                + self.measure(&row.detail, Role::Label, system, cx);
        }
        let inset = if row.nested { USER_INSET } else { 0. };
        if row.section.is_some() {
            let count = row.raw["entryCount"].to_string();
            return edges
                + inset
                + FOLD
                + GAP
                + self.measure(&row.title, Role::Heading, system, cx)
                + GAP
                + (self.measure(&count, Role::Badge, system, cx) + 14.).max(22.);
        }
        // The fold button overlaps the icon's gap by 4 px; a row that cannot
        // fold keeps 14 px in its place.
        let mut width = edges
            + inset
            + row.depth as f32 * 20.
            + 14.
            + GAP
            + 16.
            + GAP
            + self.measure(&row.title, Role::Cell, system, cx);
        if row.members.len() > 1 {
            width += GAP
                + (self.measure(&row.members.len().to_string(), Role::Badge, system, cx) + 12.)
                    .max(20.);
        }
        if let Some(note) = &row.note {
            width += GAP + (self.measure(note, Role::Label, system, cx) + 26.).max(40.);
        }
        if !row.detail.is_empty() {
            width += GAP + self.measure(&row.detail, Role::Cell, system, cx);
        }
        if row
            .members
            .iter()
            .any(|member| boolean(member, "efficiency"))
        {
            width += GAP + 12.;
        }
        if !row.members.is_empty()
            && row
                .members
                .iter()
                .all(|member| boolean(member, "suspended"))
        {
            width +=
                GAP + self.measure(&label("processes.suspended"), Role::Badge, system, cx) + 14.;
        }
        if row.pinned {
            width += GAP + 16.;
        }
        width
    }

    /// What one row needs in `field`.
    fn row_width(
        &mut self,
        index: usize,
        field: &Field,
        contents: &mut Contents<'_>,
        system: &gpui_kit::gpui::WindowTextSystem,
        cx: &App,
    ) -> Option<f32> {
        let row = contents.rows.get(index)?;
        if field.kind == CellKind::Name {
            return Some(self.name_width(row, system, cx));
        }
        let text = contents
            .values
            .entry((index, field.key))
            .or_insert_with(|| display_cell(row, field, contents.bits));
        let text = if numeric(field.kind) {
            figures(text)
        } else {
            text.clone()
        };
        Some(self.measure(&text, Role::Cell, system, cx) + CELL_PADDING * 2.)
    }

    /// What `field`'s header needs: its label with room for the sort arrow,
    /// and the total above it.
    fn header_width(
        &mut self,
        field: &Field,
        contents: &Contents<'_>,
        system: &gpui_kit::gpui::WindowTextSystem,
        cx: &App,
    ) -> (f32, f32) {
        let header =
            self.measure(&field.label, Role::Label, system, cx) + SORT_ARROW + CELL_PADDING * 2.;
        let shown = contents
            .totals
            .get(field.key)
            .map(String::as_str)
            .unwrap_or("");
        let shown = if numeric(field.kind) {
            figures(shown)
        } else {
            shown.to_owned()
        };
        let mut total = self.measure(&shown, Role::Total, system, cx);
        for text in total_envelope(field, contents.bits) {
            total = total.max(self.measure(text, Role::Total, system, cx));
        }
        // The name's total has the filter mark beside it while searching.
        let mark = if field.key == "name" { 17. } else { 0. };
        (header, total + mark + CELL_PADDING * 2.)
    }

    /// Measures `field` over every row now: what it takes to show all of
    /// it (a column fitted to its contents with automatic widths off).
    pub fn fit(&mut self, field: &Field, mut contents: Contents<'_>, cx: &App) -> f32 {
        self.epoch = self.epoch.wrapping_add(1);
        let system = gpui_kit::gpui::WindowTextSystem::new(cx.text_system().clone());
        let (header, total) = self.header_width(field, &contents, &system, cx);
        let mut required = header.max(total);
        for text in envelope(field, contents.bits) {
            required =
                required.max(self.measure(text, Role::Cell, &system, cx) + CELL_PADDING * 2.);
        }
        for index in 0..contents.rows.len() {
            if let Some(width) = self.row_width(index, field, &mut contents, &system, cx) {
                required = required.max(width);
            }
        }
        let (minimum, limit) = limits(field.key);
        stepped(required.clamp(minimum, limit.max(header).max(total)))
    }

    /// Measures the columns for the rows now shown, keeping each one's
    /// widths in `natural`.
    pub fn apply(&mut self, fields: &[Field], mut contents: Contents<'_>, cx: &App) {
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
            let (header, total) = self.header_width(field, &contents, &system, cx);
            let (minimum, limit) = limits(field.key);
            let floor = header.max(total).max(minimum);
            let ceiling = limit.max(header).max(total);
            let current = self.natural.get(field.key).copied();
            let natural = if text(field.kind) {
                // Every row now and then, so that rows scrolled to later fit
                // already; the visible ones in between can only widen it.
                if full || current.is_none() {
                    let widths = (0..count)
                        .filter_map(|index| {
                            self.row_width(index, field, &mut contents, &system, cx)
                        })
                        .collect::<Vec<_>>();
                    let all = widths.iter().copied().fold(0., f32::max);
                    let typical = stepped(typical(widths).clamp(floor, ceiling));
                    let all = stepped(all.clamp(floor, ceiling)).max(typical);
                    let typical = settled(current.map(|natural| natural.typical), typical);
                    Natural {
                        typical,
                        full: settled(current.map(|natural| natural.full), all).max(typical),
                    }
                } else {
                    let mut natural = current.unwrap_or_default();
                    for index in contents.range.clone() {
                        if let Some(width) =
                            self.row_width(index, field, &mut contents, &system, cx)
                        {
                            natural.full = natural.full.max(stepped(width.clamp(floor, ceiling)));
                        }
                    }
                    natural
                }
            } else {
                let mut required = floor;
                let bounds = envelope(field, contents.bits);
                for text in bounds {
                    required = required
                        .max(self.measure(text, Role::Cell, &system, cx) + CELL_PADDING * 2.);
                }
                // A column with an envelope checks the visible rows for
                // rarer, wider values; the others measure every row now and
                // then.
                let range = if full && bounds.is_empty() {
                    0..count
                } else {
                    contents.range.clone()
                };
                for index in range {
                    if let Some(width) = self.row_width(index, field, &mut contents, &system, cx) {
                        required = required.max(width);
                    }
                }
                let required = stepped(required.clamp(minimum, ceiling));
                Natural::fixed(current.map_or(required, |natural| natural.full.max(required)))
            };
            self.natural.insert(field.key, natural);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Natural, allocate, figures, settled, stepped, typical};

    #[test]
    fn figures_measure_alike_and_widths_grow_in_steps() {
        assert_eq!(figures("1,195.4 MB/s"), "8,888.8 MB/s");
        assert_eq!(figures("PID 4"), "PID 8");
        assert_eq!(stepped(64.), 64.);
        assert_eq!(stepped(64.5), 72.);
        assert_eq!(stepped(71.9), 72.);
    }

    #[test]
    fn a_few_long_values_are_cut_short_and_many_are_not() {
        // Most names are 150–230 px; two run much longer.
        let mut widths = (0..40)
            .map(|i| 150. + (i % 9) as f32 * 10.)
            .collect::<Vec<_>>();
        widths.extend([480., 520.]);
        assert_eq!(typical(widths), 230.);
        // Long values are no outliers when many rows have them.
        let spread = (0..40).map(|i| 100. + i as f32 * 10.).collect::<Vec<_>>();
        assert_eq!(typical(spread), 490.);
        // Nineteen in twenty always fit, even under the fence.
        let mut steep = vec![100.; 30];
        steep.extend([300., 310., 320., 330.]);
        assert!(typical(steep) >= 300.);
        assert_eq!(typical(vec![]), 0.);
        assert_eq!(typical(vec![120.]), 120.);
    }

    #[test]
    fn a_width_widens_at_once_and_narrows_past_a_step() {
        assert_eq!(settled(None, 96.), 96.);
        assert_eq!(settled(Some(96.), 104.), 104.);
        assert_eq!(settled(Some(96.), 88.), 96.);
        assert_eq!(settled(Some(96.), 80.), 80.);
    }

    #[test]
    fn text_columns_share_the_room_left_up_to_what_they_need() {
        let columns = [
            Natural {
                typical: 200.,
                full: 300.,
            },
            Natural::fixed(64.),
            Natural {
                typical: 100.,
                full: 400.,
            },
        ];
        // No room to spare: what most rows need.
        assert_eq!(allocate(&columns, 300.), [200., 64., 100.]);
        // Room for half of what they lack, shared by how much each lacks.
        assert_eq!(allocate(&columns, 564.), [250., 64., 250.]);
        // More than enough: no wider than every row needs, the rest left empty.
        assert_eq!(allocate(&columns, 2000.), [300., 64., 400.]);
    }
}
