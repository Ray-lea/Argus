//! Process column widths settled ahead of the values. A numeric column starts
//! as wide as the widest value its format shows in practice (`envelope`) and
//! only ever widens, in steps: digits measure alike, so a value changing keeps
//! its width and the columns stay put while the readings move.
//!
//! A text column (the name, a path, a description) is measured over the
//! whole list as it would be with every group open and nothing searched
//! (`WidthRows`), not over the rows shown: a search or a folded group does
//! not change what the columns take. It is as wide as most rows need
//! (`Natural::typical`), the few much longer values cut short with an
//! ellipsis, and widens toward what every row needs while the table has room
//! to spare (`allocate`). No column stretches to fill the table: what is left
//! stays empty on the right.
//!
//! The service and startup lists measure their columns the same way over the
//! whole list as loaded (`AutoWidths::apply_list`), but their last column,
//! the description or command, takes whatever room the others leave
//! (`fill`): the table always spans its width, and a column dragged
//! narrower gives its room to that last column.
use super::*;
use std::borrow::Cow;
use std::ops::Range;
use std::sync::Weak;
use std::time::Duration;

pub(super) use super::cells::CellValues;

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
    /// A command or path in the monospaced family, in the small size.
    Mono,
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
    /// The list last measured in full: another one is measured again.
    source: Weak<Vec<WidthRow>>,
}

pub(super) struct Contents<'a> {
    /// The rows shown, and those on screen.
    pub rows: &'a [Row],
    pub range: Range<usize>,
    /// The list measured in full (`WidthRows`).
    pub measured: &'a Arc<Vec<WidthRow>>,
    pub totals: &'a HashMap<String, String>,
    pub values: &'a mut CellValues,
    pub bits: bool,
}

/// The widest values a numeric format shows in practice: a column is this
/// wide from the start instead of creeping wider as readings grow. A rarer,
/// wider value still widens it.
fn envelope(field: &Field, bits: bool) -> Vec<Cow<'static, str>> {
    if field.kind == CellKind::Power {
        // Every level, in the interface language.
        return argus_core::power::LEVELS
            .iter()
            .map(|level| Cow::Owned(label(&format!("processes.power.{level}"))))
            .collect();
    }
    figures_envelope(field, bits)
        .iter()
        .map(|text| Cow::Borrowed(*text))
        .collect()
}

fn figures_envelope(field: &Field, bits: bool) -> &'static [&'static str] {
    match field.kind {
        CellKind::Percent => &["100%", "8.8%"],
        CellKind::Memory if crate::frontend::columns::byte_count(field.key) => {
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
        _ if field.kind != CellKind::Percent => figures_envelope(field, bits),
        _ => &[],
    }
}

/// Whether every row of the list is measured for `field`, not only the
/// visible ones: all but the name, which lays out a row of its own
/// (`Name`), and the figures settled by an envelope.
fn in_full(field: &Field, bits: bool) -> bool {
    field.kind != CellKind::Name && envelope(field, bits).is_empty()
}

/// What sets a name cell's layout apart (`NativeTable::name_cell`).
#[derive(Clone, Copy)]
enum NameShape {
    /// A user's heading.
    User,
    /// A category heading, with how many entries it counts.
    Section { nested: bool, count: u64 },
    Process {
        nested: bool,
        depth: usize,
        members: usize,
        efficiency: bool,
        suspended: bool,
        /// A leak alert's tag (`leaks::row_kind`).
        leak: Option<&'static str>,
    },
}

/// What a name cell lays out.
struct Name<'a> {
    shape: NameShape,
    title: &'a str,
    detail: &'a str,
    note: Option<&'a str>,
    pinned: bool,
}

impl<'a> Name<'a> {
    fn of(row: &'a Row) -> Self {
        let shape = if row.user.is_some() {
            NameShape::User
        } else if row.section.is_some() {
            NameShape::Section {
                nested: row.nested,
                count: row.raw["entryCount"].as_u64().unwrap_or(0),
            }
        } else {
            NameShape::Process {
                nested: row.nested,
                depth: row.depth,
                members: row.members.len(),
                efficiency: row
                    .members
                    .iter()
                    .any(|member| boolean(member, "efficiency")),
                suspended: !row.members.is_empty()
                    && row
                        .members
                        .iter()
                        .all(|member| boolean(member, "suspended")),
                leak: crate::frontend::leaks::row_kind(&row.members),
            }
        };
        Self {
            shape,
            title: &row.title,
            detail: &row.detail,
            note: row.note.as_deref(),
            pinned: row.pinned,
        }
    }
}

/// A row of the list the column widths measure (`WidthRows`): what its name
/// cell lays out and what each column measured in full shows, without the
/// process data the row was built from.
pub(in crate::frontend) struct WidthRow {
    shape: NameShape,
    title: String,
    detail: String,
    /// The row's marks, as `Row` has them.
    pub mark_key: Option<String>,
    pub note: Option<String>,
    pub pinned: bool,
    cells: Vec<(&'static str, SharedString)>,
}

impl WidthRow {
    /// `row` as the widths measure it in `fields`.
    pub(super) fn of(row: &Row, fields: &[Field], bits: bool) -> Self {
        Self {
            shape: Name::of(row).shape,
            title: row.title.clone(),
            detail: row.detail.clone(),
            mark_key: row.mark_key.clone(),
            note: row.note.clone(),
            pinned: row.pinned,
            cells: fields
                .iter()
                .filter(|field| in_full(field, bits))
                .map(|field| (field.key, display_cell(row, field, bits).into()))
                .collect(),
        }
    }

    fn name(&self) -> Name<'_> {
        Name {
            shape: self.shape,
            title: &self.title,
            detail: &self.detail,
            note: self.note.as_deref(),
            pinned: self.pinned,
        }
    }

    fn cell(&self, key: &str) -> Option<&SharedString> {
        self.cells
            .iter()
            .find(|(cell, _)| *cell == key)
            .map(|(_, text)| text)
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
            | CellKind::EventRate
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

/// How a column of a service or startup list takes part in sharing the
/// table's room (`fill`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Share {
    /// A width of its own: a column of a few labels, or one dragged to a width.
    Fixed(f32),
    /// Text: what most rows need, down to `min` while the last column would
    /// be narrower than its share of the room, up to what every row needs
    /// while the last column has more than its longest value needs. A
    /// width `set` by dragging gives way only when the last column would be
    /// narrower than its minimum.
    Text {
        natural: Natural,
        min: f32,
        set: bool,
    },
    /// The last column: the room the others leave, at least `min`. `full`
    /// is what its longest value needs.
    Fill { full: f32, min: f32 },
}

/// The share of the room a list's last column keeps before the text beside
/// it is cut short, unless its longest value needs less: the description or
/// command is what the list has most of to show.
const FILL_SHARE: f32 = 1. / 3.;

/// A list's widths for `room`, and how much wider each column can be
/// dragged: as far as the last column and the other text columns can give
/// way, so that the columns still span the room and nothing is pushed out
/// of the table.
///
/// The last column takes the room the others leave. When that is less than
/// its share of the room (`FILL_SHARE`), the text columns narrow toward
/// their minimums, each by its share of what they can give up; when it is
/// less than the last column's minimum, the widths set by dragging give way
/// too; past that the columns overflow the room. When the last column has
/// more than its longest value needs, the text columns widen toward what
/// all of their rows need, and the last column keeps the rest. Widths are
/// whole pixels.
pub(super) fn fill(columns: &[Share], room: f32) -> (Vec<f32>, Vec<f32>) {
    let mut widths: Vec<f32> = columns
        .iter()
        .map(|column| match *column {
            Share::Fixed(width) => width,
            Share::Text { natural, .. } => natural.typical,
            Share::Fill { .. } => 0.,
        })
        .collect();
    let Some((last, full, minimum)) =
        columns
            .iter()
            .enumerate()
            .find_map(|(index, column)| match *column {
                Share::Fill { full, min } => Some((index, full, min)),
                _ => None,
            })
    else {
        return (
            widths.iter().map(|width| width.floor()).collect(),
            vec![0.; columns.len()],
        );
    };
    let texts = || {
        columns
            .iter()
            .enumerate()
            .filter_map(|(index, column)| match *column {
                Share::Text { natural, min, set } => Some((index, natural, min, set)),
                _ => None,
            })
    };
    // Narrows the text columns (those set by dragging only when `set`) for
    // the last column to have `target`, each by its share of what they can
    // give up.
    let narrow = |widths: &mut Vec<f32>, target: f32, set: bool| {
        let rest = room - widths.iter().sum::<f32>();
        let spare: f32 = texts()
            .filter(|column| set || !column.3)
            .map(|(index, _, min, _)| (widths[index] - min).max(0.))
            .sum();
        if rest < target && spare > 0. {
            let take = ((target - rest) / spare).min(1.);
            for (index, _, min, _) in texts().filter(|column| set || !column.3) {
                widths[index] -= (widths[index] - min).max(0.) * take;
            }
        }
    };
    let rest = room - widths.iter().sum::<f32>();
    if rest > full {
        let wanted: f32 = texts()
            .map(|(_, natural, _, _)| (natural.full - natural.typical).max(0.))
            .sum();
        if wanted > 0. {
            let share = ((rest - full) / wanted).min(1.);
            for (index, natural, _, _) in texts() {
                widths[index] = natural.typical + (natural.full - natural.typical).max(0.) * share;
            }
        }
    } else {
        narrow(
            &mut widths,
            (room * FILL_SHARE).min(full).max(minimum),
            false,
        );
        narrow(&mut widths, minimum, true);
    }
    for (index, width) in widths.iter_mut().enumerate() {
        if index != last {
            *width = width.floor();
        }
    }
    let others: f32 = widths.iter().sum();
    widths[last] = (room - others).floor().max(minimum);
    // What each column can be dragged wider by: the last column down to its
    // minimum, then the other text columns down to theirs.
    let yield_last = widths[last] - minimum;
    let yields: Vec<f32> = columns
        .iter()
        .zip(&widths)
        .map(|(column, width)| match *column {
            Share::Text { min, .. } => (width - min).max(0.),
            _ => 0.,
        })
        .collect();
    let total: f32 = yields.iter().sum();
    let reach = yields
        .iter()
        .enumerate()
        .map(|(index, own)| {
            if index == last {
                0.
            } else {
                yield_last + total - own
            }
        })
        .collect();
    (widths, reach)
}

/// What a list column's header takes besides its label: the sort arrow, the
/// cell's padding, the table's inset at its right (`HEADER_INSET`) and a
/// filter's button in place of the right padding.
pub(super) fn header_extra(page: &str, key: &str) -> f32 {
    let right = if column_filter(page, key).is_some() {
        FILTER_BUTTON + 4.
    } else {
        CELL_PADDING
    };
    CELL_PADDING + SORT_ARROW + HEADER_INSET + right
}

impl AutoWidths {
    /// Measures every column anew (another text size or language).
    pub fn reset(&mut self) {
        self.natural.clear();
        self.measured.clear();
        self.source = Weak::new();
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
            Role::Mono => 12.,
        });
        let mut run = theme::text_run(&text, theme::LABEL_WEIGHT, cx);
        if role == Role::Total {
            run.font.family = theme::display_font(cx);
        } else if role == Role::Mono {
            run.font.family = theme::mono_font(cx);
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

    /// What a name cell takes, laid out as `NativeTable::name_cell` draws
    /// it: the indent, the fold button or its space, the icon, the name and
    /// whatever follows it.
    fn name_width(
        &mut self,
        name: Name<'_>,
        system: &gpui_kit::gpui::WindowTextSystem,
        cx: &App,
    ) -> f32 {
        const GAP: f32 = 8.;
        const FOLD: f32 = 18.;
        let edges = CELL_PADDING * 2. + 6.;
        let inset = |nested: bool| if nested { USER_INSET } else { 0. };
        let (nested, depth, members, efficiency, suspended, leak) = match name.shape {
            NameShape::User => {
                return edges
                    + FOLD
                    + GAP
                    + 16.
                    + GAP
                    + self.measure(name.title, Role::Heading, system, cx)
                    + GAP
                    + self.measure(name.detail, Role::Label, system, cx);
            }
            NameShape::Section { nested, count } => {
                return edges
                    + inset(nested)
                    + FOLD
                    + GAP
                    + self.measure(name.title, Role::Heading, system, cx)
                    + GAP
                    + (self.measure(&count.to_string(), Role::Badge, system, cx) + 14.).max(22.);
            }
            NameShape::Process {
                nested,
                depth,
                members,
                efficiency,
                suspended,
                leak,
            } => (nested, depth, members, efficiency, suspended, leak),
        };
        // The fold button overlaps the icon's gap by 4 px; a row that cannot
        // fold keeps 14 px in its place.
        let mut width = edges
            + inset(nested)
            + depth as f32 * 20.
            + 14.
            + GAP
            + 16.
            + GAP
            + self.measure(name.title, Role::Cell, system, cx);
        if members > 1 {
            width +=
                GAP + (self.measure(&members.to_string(), Role::Badge, system, cx) + 12.).max(20.);
        }
        if let Some(note) = name.note {
            width += GAP + (self.measure(note, Role::Label, system, cx) + 26.).max(40.);
        }
        if !name.detail.is_empty() {
            width += GAP + self.measure(name.detail, Role::Cell, system, cx);
        }
        if efficiency {
            width += GAP + 12.;
        }
        if suspended {
            width +=
                GAP + self.measure(&label("processes.suspended"), Role::Badge, system, cx) + 14.;
        }
        if let Some(kind) = leak {
            width +=
                GAP + self.measure(
                    &crate::frontend::leaks::tag_label(kind),
                    Role::Badge,
                    system,
                    cx,
                ) + 14.;
        }
        if name.pinned {
            width += GAP + 16.;
        }
        width
    }

    /// What a cell showing `text` in `field` needs.
    fn text_width(
        &mut self,
        field: &Field,
        text: &str,
        system: &gpui_kit::gpui::WindowTextSystem,
        cx: &App,
    ) -> f32 {
        let text = if numeric(field.kind) {
            figures(text)
        } else {
            text.to_owned()
        };
        self.measure(&text, Role::Cell, system, cx) + CELL_PADDING * 2.
    }

    /// What the row shown at `index` needs in `field`; the text is kept
    /// for the cell to draw.
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
            return Some(self.name_width(Name::of(row), system, cx));
        }
        let text = contents.values.get(index, row, field).text.clone();
        Some(self.text_width(field, &text, system, cx))
    }

    /// What each row of the measured list needs in `field`. A column chosen
    /// since the list was built has no text there: the rows shown stand in
    /// until the next.
    fn measured_widths(
        &mut self,
        field: &Field,
        contents: &mut Contents<'_>,
        system: &gpui_kit::gpui::WindowTextSystem,
        cx: &App,
    ) -> Vec<f32> {
        let measured = contents.measured;
        let widths = measured
            .iter()
            .filter_map(|row| {
                if field.kind == CellKind::Name {
                    return Some(self.name_width(row.name(), system, cx));
                }
                let text = row.cell(field.key)?;
                Some(self.text_width(field, text, system, cx))
            })
            .collect::<Vec<_>>();
        if !widths.is_empty() {
            return widths;
        }
        (0..contents.rows.len())
            .filter_map(|index| self.row_width(index, field, contents, system, cx))
            .collect()
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
        contents.values.prepare(cx);
        self.epoch = self.epoch.wrapping_add(1);
        let system = gpui_kit::gpui::WindowTextSystem::new(cx.text_system().clone());
        let (header, total) = self.header_width(field, &contents, &system, cx);
        let mut required = header.max(total);
        for text in envelope(field, contents.bits) {
            required =
                required.max(self.measure(&text, Role::Cell, &system, cx) + CELL_PADDING * 2.);
        }
        for index in 0..contents.rows.len() {
            if let Some(width) = self.row_width(index, field, &mut contents, &system, cx) {
                required = required.max(width);
            }
        }
        let (minimum, limit) = limits(field.key);
        stepped(required.clamp(minimum, limit.max(header).max(total)))
    }

    /// Measures the columns, keeping each one's widths in `natural`: every
    /// row of a list not measured yet, else the rows on screen, which can
    /// only widen a column. A search or a fold changes the rows shown, not
    /// the measured list, and so leaves the widths as they are.
    pub fn apply(&mut self, fields: &[Field], mut contents: Contents<'_>, cx: &App) {
        self.epoch = self.epoch.wrapping_add(1);
        let system = gpui_kit::gpui::WindowTextSystem::new(cx.text_system().clone());
        let full = !std::ptr::eq(self.source.as_ptr(), Arc::as_ptr(contents.measured));
        if full {
            self.source = Arc::downgrade(contents.measured);
        }
        for field in fields {
            let (header, total) = self.header_width(field, &contents, &system, cx);
            let (minimum, limit) = limits(field.key);
            let floor = header.max(total).max(minimum);
            let ceiling = limit.max(header).max(total);
            let current = self.natural.get(field.key).copied();
            let natural = if text(field.kind) {
                // Every row of each new list, so that rows scrolled to,
                // unfolded or searched for later fit already; the visible
                // ones in between can only widen it.
                if full || current.is_none() {
                    let widths = self.measured_widths(field, &mut contents, &system, cx);
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
                for text in &bounds {
                    required = required
                        .max(self.measure(text, Role::Cell, &system, cx) + CELL_PADDING * 2.);
                }
                // A column with an envelope checks the visible rows for
                // rarer, wider values; the others measure every row of each
                // new list too.
                if (full || current.is_none()) && bounds.is_empty() {
                    for width in self.measured_widths(field, &mut contents, &system, cx) {
                        required = required.max(width);
                    }
                }
                for index in contents.range.clone() {
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

    /// What a service or startup entry's name cell takes, laid out as
    /// `NativeTable::name_cell` draws it: a startup entry's icon before the
    /// name, and its mark for a missing file after it.
    fn entry_name_width(
        &mut self,
        page: &str,
        entry: &Value,
        system: &gpui_kit::gpui::WindowTextSystem,
        cx: &App,
    ) -> f32 {
        const GAP: f32 = 8.;
        let title = super::text(
            entry,
            if page == "services" {
                "displayName"
            } else {
                "name"
            },
        );
        let mut width = CELL_PADDING * 2. + self.measure(title, Role::Cell, system, cx);
        if page == "startup" {
            width += 16. + GAP;
            if !boolean(entry, "exists") && !entry["path"].is_null() {
                // The mark's padding and border.
                let mark = label("startup.missing.label");
                width += GAP + self.measure(&mark, Role::Badge, system, cx) + 14.;
            }
        }
        width
    }

    /// Measures the columns of a service or startup list over `entries`, the
    /// list as loaded (nothing searched or filtered, so that neither changes
    /// the widths), keeping each one's widths in `natural`. Names and other
    /// free text take what most rows need (`Natural::typical`); labels and
    /// figures what every row needs; the last column (`Track::Fill`) records
    /// what its longest value needs.
    pub fn apply_list(&mut self, page: &str, fields: &[Field], entries: &[Value], cx: &App) {
        self.epoch = self.epoch.wrapping_add(1);
        let system = gpui_kit::gpui::WindowTextSystem::new(cx.text_system().clone());
        for field in fields {
            let (minimum, limit) = match field.track {
                Track::Fit(minimum, limit) => (minimum, limit),
                Track::Fill(minimum) => (minimum, f32::INFINITY),
                Track::Fixed(_) | Track::Flex(..) => continue,
            };
            let header = self.measure(&field.label, Role::Label, &system, cx)
                + header_extra(page, field.key);
            let widths = entries
                .iter()
                .map(|entry| {
                    if field.kind == CellKind::Name {
                        return self.entry_name_width(page, entry, &system, cx);
                    }
                    let shown = entry_cell(entry, field);
                    let (shown, role) = match field.kind {
                        CellKind::Mono | CellKind::MonoDim => (shown, Role::Mono),
                        kind if numeric(kind) || kind == CellKind::ServicePid => {
                            (figures(&shown), Role::Cell)
                        }
                        _ => (shown, Role::Cell),
                    };
                    self.measure(&shown, role, &system, cx) + CELL_PADDING * 2.
                })
                .collect::<Vec<_>>();
            let all = widths.iter().copied().fold(0., f32::max);
            let floor = header.max(minimum);
            let ceiling = limit.max(header);
            let current = self.natural.get(field.key).copied();
            let natural = match field.track {
                Track::Fill(_) => Natural::fixed(all.max(floor).ceil()),
                // A few much longer names or descriptions are cut short.
                _ if matches!(field.kind, CellKind::Name | CellKind::Dim) => {
                    let typical = stepped(typical(widths).clamp(floor, ceiling));
                    let all = stepped(all.clamp(floor, ceiling)).max(typical);
                    let typical = settled(current.map(|natural| natural.typical), typical);
                    Natural {
                        typical,
                        full: settled(current.map(|natural| natural.full), all).max(typical),
                    }
                }
                _ => Natural::fixed(settled(
                    current.map(|natural| natural.full),
                    stepped(all.clamp(floor, ceiling)),
                )),
            };
            self.natural.insert(field.key, natural);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Natural, Share, allocate, figures, fill, settled, stepped, typical};

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

    #[test]
    fn the_last_list_column_takes_the_room_the_others_leave() {
        let name = Share::Text {
            natural: Natural {
                typical: 240.,
                full: 320.,
            },
            min: 160.,
            set: false,
        };
        let key = Share::Text {
            natural: Natural {
                typical: 160.,
                full: 200.,
            },
            min: 100.,
            set: false,
        };
        let columns = [
            name,
            key,
            Share::Fixed(90.),
            Share::Fill {
                full: 900.,
                min: 240.,
            },
        ];
        // The others as most rows need them, the description the rest; it
        // can give way down to its minimum, the text columns to theirs.
        let (widths, reach) = fill(&columns, 1200.);
        assert_eq!(widths, [240., 160., 90., 710.]);
        assert_eq!(reach, [470. + 60., 470. + 80., 470. + 140., 0.]);
        // A column dragged narrower gives its room to the last column only.
        let dragged = [Share::Fixed(180.), key, Share::Fixed(90.), columns[3]];
        assert_eq!(fill(&dragged, 1200.).0, [180., 160., 90., 770.]);
        // Short of room: the text columns narrow, each by its share of what
        // it can give up, for the last column to keep its share of the
        // room, and to its minimum before the columns overflow.
        let wide = |typical: f32, min: f32| Share::Text {
            natural: Natural {
                typical,
                full: typical,
            },
            min,
            set: false,
        };
        let long = [
            wide(500., 160.),
            wide(300., 100.),
            Share::Fixed(90.),
            columns[3],
        ];
        assert_eq!(fill(&long, 1200.).0, [443., 266., 90., 401.]);
        let (widths, _) = fill(&columns, 640.);
        assert_eq!(widths, [188., 121., 90., 241.]);
        let (widths, _) = fill(&columns, 500.);
        assert_eq!(widths, [160., 100., 90., 240.]);
        // More than the last column needs: the text columns widen toward
        // what every row needs, the last column keeps the rest.
        let (widths, _) = fill(&columns, 1600.);
        assert_eq!(widths, [320., 200., 90., 990.]);
        let (widths, _) = fill(&columns, 1500.);
        assert_eq!(widths, [313., 196., 90., 901.]);
        // A width set by dragging gives way only for the last column's minimum.
        let set = Share::Text {
            natural: Natural::fixed(300.),
            min: 160.,
            set: true,
        };
        let columns = [set, key, Share::Fixed(90.), columns[3]];
        assert_eq!(fill(&columns, 800.).0, [300., 143., 90., 267.]);
        assert_eq!(fill(&columns, 700.).0, [270., 100., 90., 240.]);
    }
}
