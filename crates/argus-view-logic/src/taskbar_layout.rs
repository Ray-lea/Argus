//! What the taskbar monitor shows, laid out in one of four styles
//! (`TaskbarStyle`):
//!
//! - labels: short readings in columns of two rows (one on a taskbar too low
//!   for two), each a name in its device's colour on the left and its value
//!   on the right;
//! - curves: a tile per reading, its name and value over its recent curve in
//!   its device's colour;
//! - capsules: a rounded capsule per reading in one row, a dot in its
//!   device's colour before its name and value;
//! - figures: a small name in its device's colour over a large figure.
//!
//! On the taskbar a value keeps the room of the widest it took within the
//! last minute (`Reserve`), so that changing values seldom move the
//! readings or resize the window, without the room of a value it hardly
//! ever takes; a curve's tile, and every value laid out without a reserve
//! (the panel's previews), keep the room of the widest it may get
//! (`Cell::widest`). The sizes follow the taskbar's height (`size_scale`).
//! Curves, capsules and figures need two rows of text; a lower taskbar
//! shows labels.
//!
//! A vertical taskbar stacks the readings down its width instead, in each
//! of the four styles (`lay_out_column`, in `column`). Where the taskbar
//! has less room than the readings take, only the first ones that fit show
//! (`lay_out_within`, and the room `lay_out_column` takes).
//!
//! The layout measures text through `Fonts`; the interface sets and draws it
//! with DirectWrite (`shell/taskbar_draw.rs` in `src-gpui`, its `Painter`).
use argus_app::settings::{TaskbarMonitor, TaskbarSize, TaskbarStyle as Style};
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

mod column;
pub use column::lay_out_column;

/// One reading on the taskbar.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Cell {
    /// Its short name: "CPU", or an arrow for a rate.
    pub label: String,
    /// Its parameter's name ("Temp") when it follows a reading of the same
    /// device, which names the device; empty otherwise. It stands for
    /// `label` where the device shows anyway: by colour, or by that
    /// reading beside it (`Cell::tag`).
    pub name: String,
    /// Readings of one device in a row share this; others differ.
    pub run: usize,
    /// The reading it shows, as saved ("memory", "disk@…"): where it keeps
    /// its room, wherever it moves among the others.
    pub key: String,
    pub value: String,
    /// The widest value it takes, so that it keeps its room.
    pub widest: String,
    /// High enough to show amber.
    pub high: bool,
}

impl Cell {
    /// What it is called where its device's colour marks it: its
    /// parameter's name after a reading of its device, else its short name.
    fn tag(&self) -> &str {
        if self.name.is_empty() {
            &self.label
        } else {
            &self.name
        }
    }

    /// Where it keeps its room in `style`: its reading and its mark (a
    /// pair's two rates differ by theirs), not its place, which another
    /// reading takes when one is added, removed or moved.
    fn kept(&self, style: &str) -> String {
        format!("{style}:{}:{}", self.key, self.label)
    }

    /// What it is called in plain text after `before` (the cell above it,
    /// or to its left in a single row): its parameter's name only next to
    /// a reading of its device.
    fn tag_after(&self, before: Option<&Cell>) -> &str {
        if before.is_some_and(|before| before.run == self.run) {
            self.tag()
        } else {
            &self.label
        }
    }
}

/// The device a reading is of, for its colour.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tone {
    #[default]
    Plain,
    Cpu,
    Memory,
    Gpu,
    Disk,
    Network,
    Battery,
}

/// What one metric shows: one cell, or the two of a pair of rates (disk,
/// network), which keep together; its device, and its recent values for a
/// curve (0–1, oldest first; empty without a history).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Group {
    pub cells: Vec<Cell>,
    pub tone: Tone,
    pub history: Vec<f32>,
}

/// Arranges the groups in columns of `rows` cells, each cell with the group
/// it is of: a group's cells stay together, and single cells share a column
/// in order.
fn columns(groups: &[Group], rows: usize) -> Vec<Vec<(&Cell, &Group)>> {
    let mut columns: Vec<Vec<(&Cell, &Group)>> = Vec::new();
    // The column a single cell opened, which the next single cell fills.
    let mut open: Option<usize> = None;
    for group in groups {
        if rows < 2 {
            columns.extend(group.cells.iter().map(|cell| vec![(cell, group)]));
            continue;
        }
        match group.cells.as_slice() {
            [] => {}
            [cell] => match open.take() {
                Some(index) => columns[index].push((cell, group)),
                None => {
                    open = Some(columns.len());
                    columns.push(vec![(cell, group)]);
                }
            },
            cells => columns.extend(
                cells
                    .chunks(rows)
                    .map(|chunk| chunk.iter().map(|cell| (cell, group)).collect()),
            ),
        }
    }
    columns
}

/// What a layout measures text with.
pub trait Fonts {
    /// The width of `text` set `size` pixels high, in a value's weight or a
    /// name's.
    fn width(&self, text: &str, size: i32, value: bool) -> i32;
    /// The capitals' height at `size`.
    fn cap(&self, size: i32) -> f32;
}

/// The colour of a run: the text's own, amber, or its device's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Paint {
    Ink,
    High,
    Tone(Tone),
}

/// How strongly a run shows: a value in full, a name quieter, a unit
/// quieter still.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shade {
    Full,
    Name,
    Unit,
}

/// Where a text goes, in pixels of the image.
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub text: String,
    pub x: i32,
    pub baseline: i32,
    /// Its size, pixels.
    pub size: i32,
    /// Set in the value's weight rather than the name's.
    pub value: bool,
    pub paint: Paint,
    pub shade: Shade,
}

/// A tile behind a reading, with its curve.
#[derive(Clone, Debug, PartialEq)]
pub struct Tile {
    pub rect: [i32; 4],
    /// Where its curve goes.
    pub curve: [i32; 4],
    pub history: Vec<f32>,
    pub paint: Paint,
}

/// A capsule behind a reading, and the dot that marks its device.
#[derive(Clone, Debug, PartialEq)]
pub struct Pill {
    pub rect: [i32; 4],
    /// The dot's centre and radius, none for a rate (its arrow marks it).
    pub dot: Option<([f32; 2], f32)>,
    pub paint: Paint,
}

/// The monitor laid out at one size.
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub width: i32,
    pub height: i32,
    pub runs: Vec<Run>,
    pub tiles: Vec<Tile>,
    pub pills: Vec<Pill>,
    /// The labels' text size, pixels.
    pub font: i32,
    /// How much of the text and tiles shows, percent; the taskbar's own
    /// background shows through the rest.
    pub opacity: u8,
    /// Laid out down a vertical taskbar (`lay_out_column`), its width the
    /// taskbar's, rather than along a horizontal one.
    pub vertical: bool,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            width: 0,
            height: 0,
            runs: Vec::new(),
            tiles: Vec::new(),
            pills: Vec::new(),
            font: 0,
            opacity: 100,
            vertical: false,
        }
    }
}

/// How the monitor shows its readings: the style, the text's size, the
/// labels' own choices (one row or two on a horizontal taskbar, values in
/// their devices' colours), and its opacity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Look {
    pub style: Style,
    pub size: TaskbarSize,
    /// One row of labels or two on a horizontal taskbar; the other styles
    /// ignore it.
    pub rows: u8,
    /// Labels' values in their device's colour rather than the taskbar's.
    pub colored: bool,
    /// How much of the text and tiles shows, percent; one of the
    /// interface's monitor opacities.
    pub opacity: u8,
}

impl Default for Look {
    fn default() -> Self {
        Self {
            style: Style::default(),
            size: TaskbarSize::default(),
            rows: 2,
            colored: false,
            opacity: 100,
        }
    }
}

impl From<&TaskbarMonitor> for Look {
    fn from(monitor: &TaskbarMonitor) -> Self {
        Self {
            style: monitor.style,
            size: monitor.size,
            rows: monitor.rows,
            colored: monitor.colored,
            opacity: monitor.opacity,
        }
    }
}

/// The labels' text size, pixels, at `scale` as `size_scale` gives it:
/// 15 px on Windows 11's 48 px taskbar, where two rows of it fill the
/// height but its margins; 13 px, the interface's own, on Windows 10's
/// 40 px one. At the clock's 12 px the readings were hard to make out.
pub fn font_size(scale: f32) -> i32 {
    (15. * scale).round().max(8.) as i32
}

/// How large the readings are set on a taskbar `height` pixels high at
/// `scale`: the styles' sizes are for Windows 11's 48 px taskbar, and
/// follow a lower or higher one within bounds, so that they fill its
/// height rather than leave it empty around small text.
pub fn size_scale(height: i32, scale: f32) -> f32 {
    scale * (height as f32 / (48. * scale)).clamp(0.85, 1.25)
}

/// A value's figure and its unit: "12.4" and " MB/s", the unit drawn
/// smaller and quieter. A value with more figures after its first unit (an
/// uptime) stays whole; an unknown value's dash counts as a figure.
fn split(value: &str) -> (&str, &str) {
    let at = value
        .find(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | ',' | '—')))
        .unwrap_or(value.len());
    let (figure, unit) = value.split_at(at);
    if unit.chars().any(|c| c.is_ascii_digit()) {
        (value, "")
    } else {
        (figure, unit)
    }
}

/// The room a value takes as `figure` lays it out.
fn figure_width(fonts: &impl Fonts, value: &str, size: i32, unit: i32) -> i32 {
    let (figure, rest) = split(value);
    fonts.width(figure, size, true) + fonts.width(rest, unit, true)
}

/// Lays out a value ending at `right`: its figure `size` pixels high, its
/// unit `unit` pixels high and quieter.
#[allow(clippy::too_many_arguments)]
fn figure(
    runs: &mut Vec<Run>,
    fonts: &impl Fonts,
    value: &str,
    right: i32,
    baseline: i32,
    size: i32,
    unit: i32,
    paint: Paint,
) {
    let (figure, rest) = split(value);
    let rest_width = fonts.width(rest, unit, true);
    if !rest.is_empty() {
        runs.push(Run {
            text: rest.into(),
            x: right - rest_width,
            baseline,
            size: unit,
            value: true,
            paint,
            shade: Shade::Unit,
        });
    }
    runs.push(Run {
        text: figure.into(),
        x: right - rest_width - fonts.width(figure, size, true),
        baseline,
        size,
        value: true,
        paint,
        shade: Shade::Full,
    });
}

/// Lays out a value from `left`: its figure `size` pixels high, then its
/// unit `unit` pixels high and quieter.
#[allow(clippy::too_many_arguments)]
fn figure_from(
    runs: &mut Vec<Run>,
    fonts: &impl Fonts,
    value: &str,
    left: i32,
    baseline: i32,
    size: i32,
    unit: i32,
    paint: Paint,
) {
    let (figure, rest) = split(value);
    runs.push(Run {
        text: figure.into(),
        x: left,
        baseline,
        size,
        value: true,
        paint,
        shade: Shade::Full,
    });
    if !rest.is_empty() {
        runs.push(Run {
            text: rest.into(),
            x: left + fonts.width(figure, size, true),
            baseline,
            size: unit,
            value: true,
            paint,
            shade: Shade::Unit,
        });
    }
}

fn value_paint(cell: &Cell) -> Paint {
    if cell.high { Paint::High } else { Paint::Ink }
}

pub(super) fn label_value_paint(cell: &Cell, tone: Tone, colored: bool) -> Paint {
    if cell.high {
        Paint::High
    } else if colored && tone != Tone::Plain {
        Paint::Tone(tone)
    } else {
        Paint::Ink
    }
}

/// How wide each capsule's values have been lately: a capsule keeps the
/// room its value took within the last minute, widening at once for a wider
/// value and narrowing a minute after it, rather than the room of the
/// widest value it may ever take.
#[derive(Default)]
pub struct Reserve {
    font: i32,
    widths: HashMap<String, VecDeque<(Instant, i32)>>,
}

/// How long a value's width holds its room.
const KEEP: Duration = Duration::from_secs(60);

impl Reserve {
    /// Drops widths measured at another font size (a change of text size,
    /// DPI or taskbar thickness).
    pub(super) fn at_font(&mut self, font: i32) {
        if self.font != font {
            self.font = font;
            self.widths.clear();
        }
    }

    /// The room for the value `key` names, `width` wide `now`.
    fn room(&mut self, key: String, width: i32, now: Instant) -> i32 {
        let seen = self.widths.entry(key).or_default();
        match seen.back_mut() {
            Some(last) if last.1 == width => last.0 = now,
            _ => seen.push_back((now, width)),
        }
        seen.retain(|(at, _)| now.saturating_duration_since(*at) <= KEEP);
        seen.iter().map(|(_, width)| *width).max().unwrap_or(width)
    }
}

/// The room for a value that is `width` wide and may get `widest` wide:
/// the room it took lately, kept in `reserve` under `key` when given one,
/// its widest without.
fn held(
    reserve: &mut Option<(&mut Reserve, Instant)>,
    key: String,
    width: i32,
    widest: i32,
) -> i32 {
    match reserve.as_mut() {
        Some((reserve, now)) => reserve.room(key, width, *now),
        None => width.max(widest),
    }
}

/// Lays out `groups` in `look` in a window `height` pixels high at
/// `scale`, its sizes following the height (`size_scale`); labels,
/// capsules and figures keep their recent room in `reserve` when given
/// one, at the time given with it, and the room of their widest values
/// without.
pub fn lay_out(
    groups: &[Group],
    height: i32,
    scale: f32,
    look: Look,
    fonts: &impl Fonts,
    reserve: Option<(&mut Reserve, Instant)>,
) -> Layout {
    lay_out_within(groups, height, scale, look, fonts, reserve, i32::MAX)
}

/// Lays out `groups` as `lay_out` does, showing only the first readings
/// that fit in `room` pixels along the taskbar: whole columns of labels,
/// whole readings in the other styles. None fitting leaves the layout empty
/// (no width).
pub fn lay_out_within(
    groups: &[Group],
    height: i32,
    scale: f32,
    look: Look,
    fonts: &impl Fonts,
    mut reserve: Option<(&mut Reserve, Instant)>,
    room: i32,
) -> Layout {
    let low = height < (36. * scale).round() as i32;
    let style = if low { Style::Labels } else { look.style };
    let bar_scale = scale;
    let scale = size_scale(height, scale) * look.size.factor();
    let mut layout = Layout {
        height,
        font: font_size(scale),
        opacity: look.opacity.clamp(60, 100),
        ..Layout::default()
    };
    if let Some((reserve, _)) = reserve.as_mut() {
        reserve.at_font(layout.font);
    }
    let mut ends = Vec::new();
    layout.width = match style {
        Style::Labels => labels(
            &mut layout,
            &mut ends,
            groups,
            scale,
            bar_scale,
            low,
            look,
            fonts,
            reserve,
        ),
        Style::Curves => curves(&mut layout, &mut ends, groups, scale, fonts),
        Style::Capsules => capsules(&mut layout, &mut ends, groups, scale, fonts, reserve),
        Style::Figures => figures(&mut layout, &mut ends, groups, scale, fonts, reserve),
    };
    // The padding after the last reading, as after any.
    let Some(&(last, ..)) = ends.last() else {
        return layout;
    };
    let pad = layout.width - last;
    let fit = ends
        .iter()
        .take_while(|(end, ..)| end + pad <= room)
        .count();
    match fit.checked_sub(1).map(|index| ends[index]) {
        Some((end, runs, tiles, pills)) => {
            layout.runs.truncate(runs);
            layout.tiles.truncate(tiles);
            layout.pills.truncate(pills);
            layout.width = end + pad;
        }
        None => {
            layout.runs.clear();
            layout.tiles.clear();
            layout.pills.clear();
            layout.width = 0;
        }
    }
    layout
}

/// Where a column of labels, or a reading in the other styles, ends along
/// the taskbar, and how many runs, tiles and capsules were laid out by then.
type End = (i32, usize, usize, usize);

/// Notes that a column or a reading ends at `x`.
fn ended(ends: &mut Vec<End>, x: i32, layout: &Layout) {
    ends.push((x, layout.runs.len(), layout.tiles.len(), layout.pills.len()));
}

/// Labels: names and values in columns of two rows (or one); the layout's
/// width.
#[allow(clippy::too_many_arguments)]
fn labels(
    layout: &mut Layout,
    ends: &mut Vec<End>,
    groups: &[Group],
    scale: f32,
    bar_scale: f32,
    low: bool,
    look: Look,
    fonts: &impl Fonts,
    mut reserve: Option<(&mut Reserve, Instant)>,
) -> i32 {
    let px = |logical: f32| (logical * scale).round() as i32;
    let (font, height) = (layout.font, layout.height);
    let rows = if low {
        1
    } else {
        look.rows.clamp(1, 2) as usize
    };
    let margin = (4. * bar_scale).round() as i32;
    let line =
        ((font as f32 * 4. / 3.).round() as i32).min((height - margin).max(font) / rows as i32);
    // The capitals' middle on the row's middle.
    let baseline = ((line as f32 + fonts.cap(font)) / 2.).round() as i32;
    let (pad, inner, gap) = (px(8.), px(7.), px(12.));
    let mut x = pad;
    let columns = columns(groups, rows);
    for (index, column) in columns.iter().enumerate() {
        if index > 0 {
            x += gap;
        }
        // A parameter goes by its own name only under (or in one row,
        // after) a reading of its device: a column's first name names its
        // device.
        let tags = column
            .iter()
            .enumerate()
            .map(|(row, (cell, _))| {
                let before = match row {
                    0 if rows == 1 && index > 0 => columns[index - 1].last().map(|(cell, _)| *cell),
                    0 => None,
                    row => Some(column[row - 1].0),
                };
                cell.tag_after(before).to_owned()
            })
            .collect::<Vec<_>>();
        let names = tags
            .iter()
            .map(|tag| fonts.width(tag, font, false))
            .collect::<Vec<_>>();
        let values = column
            .iter()
            .map(|(cell, _)| fonts.width(&cell.value, font, true))
            .collect::<Vec<_>>();
        let width = column
            .iter()
            .zip(&names)
            .zip(&values)
            .enumerate()
            .map(|(row, (((cell, _), name), value))| {
                let room = held(
                    &mut reserve,
                    cell.kept("labels"),
                    *value,
                    fonts.width(&cell.widest, font, true),
                );
                if tags[row].is_empty() {
                    room
                } else {
                    name + inner + room
                }
            })
            .max()
            .unwrap_or(0);
        // A lone cell sits in the middle of the column's height.
        let top = (height - column.len() as i32 * line) / 2;
        for (row, (((cell, group), tag), value)) in
            column.iter().zip(&tags).zip(&values).enumerate()
        {
            let y = top + row as i32 * line + baseline;
            if !tag.is_empty() {
                // In its device's colour even when the value is amber, as a
                // figure's name. A reading of no device (an uptime) has no
                // colour to take: its name stays a plain one's quieter ink,
                // apart from its value of the same size.
                let (paint, shade) = match group.tone {
                    Tone::Plain => (Paint::Ink, Shade::Name),
                    tone => (Paint::Tone(tone), Shade::Full),
                };
                layout.runs.push(Run {
                    text: tag.clone(),
                    x,
                    baseline: y,
                    size: font,
                    value: false,
                    paint,
                    shade,
                });
            }
            layout.runs.push(Run {
                text: cell.value.clone(),
                x: x + width - value,
                baseline: y,
                size: font,
                value: true,
                paint: label_value_paint(cell, group.tone, look.colored),
                shade: Shade::Full,
            });
        }
        x += width;
        ended(ends, x, layout);
    }
    if columns.is_empty() { 0 } else { x + pad }
}

/// Curves: a tile per reading, its name and value over its recent curve; a
/// pair of rates in two rows over theirs. The layout's width.
fn curves(
    layout: &mut Layout,
    ends: &mut Vec<End>,
    groups: &[Group],
    scale: f32,
    fonts: &impl Fonts,
) -> i32 {
    let px = |logical: f32| (logical * scale).round() as i32;
    let (small, body) = (px(12.), px(14.));
    let tall = px(40.).min(layout.height);
    let top = (layout.height - tall) / 2;
    let bottom = top + tall;
    let (pad, inside, gap) = (px(8.), px(9.), px(6.));
    let mut x = pad;
    for group in groups.iter().filter(|group| !group.cells.is_empty()) {
        if !layout.tiles.is_empty() {
            x += gap;
        }
        let high = group.cells.iter().any(|cell| cell.high);
        let paint = if high {
            Paint::High
        } else {
            Paint::Tone(group.tone)
        };
        let (width, curve) = match group.cells.as_slice() {
            [cell] => {
                let name = fonts.width(cell.tag(), small, false);
                let room = figure_width(fonts, &cell.widest, body, small).max(figure_width(
                    fonts,
                    &cell.value,
                    body,
                    small,
                ));
                let width = inside + name + px(9.) + room + inside;
                let baseline = top + px(15.);
                layout.runs.push(Run {
                    text: cell.tag().to_owned(),
                    x: x + inside,
                    baseline,
                    size: small,
                    value: false,
                    paint: Paint::Ink,
                    shade: Shade::Name,
                });
                figure(
                    &mut layout.runs,
                    fonts,
                    &cell.value,
                    x + width - inside,
                    baseline,
                    body,
                    small,
                    value_paint(cell),
                );
                (width, top + px(20.))
            }
            cells => {
                let marks = cells
                    .iter()
                    .map(|cell| fonts.width(&cell.label, small, true))
                    .max()
                    .unwrap_or(0);
                let room = cells
                    .iter()
                    .map(|cell| {
                        figure_width(fonts, &cell.widest, small, small).max(figure_width(
                            fonts,
                            &cell.value,
                            small,
                            small,
                        ))
                    })
                    .max()
                    .unwrap_or(0);
                let width = inside + marks + px(6.) + room + inside;
                for (row, cell) in cells.iter().take(2).enumerate() {
                    let baseline = top + [px(13.), px(26.)][row];
                    layout.runs.push(Run {
                        text: cell.label.clone(),
                        x: x + inside,
                        baseline,
                        size: small,
                        value: true,
                        paint: Paint::Tone(group.tone),
                        shade: Shade::Full,
                    });
                    figure(
                        &mut layout.runs,
                        fonts,
                        &cell.value,
                        x + width - inside,
                        baseline,
                        small,
                        small,
                        value_paint(cell),
                    );
                }
                (width, top + px(29.))
            }
        };
        layout.tiles.push(Tile {
            rect: [x, top, x + width, bottom],
            curve: [x + 1, curve, x + width - 1, bottom - 1],
            history: group.history.clone(),
            paint,
        });
        x += width;
        ended(ends, x, layout);
    }
    if layout.tiles.is_empty() { 0 } else { x + pad }
}

/// Capsules: a rounded capsule per reading, side by side: a dot in its
/// device's colour, its name and its value right after it; a pair of rates
/// share one, each right after its arrow in its device's colour. The
/// layout's width.
fn capsules(
    layout: &mut Layout,
    ends: &mut Vec<End>,
    groups: &[Group],
    scale: f32,
    fonts: &impl Fonts,
    mut reserve: Option<(&mut Reserve, Instant)>,
) -> i32 {
    let px = |logical: f32| (logical * scale).round() as i32;
    let (name_size, value_size, unit) = (px(13.), px(14.), px(12.));
    let tall = px(30.).min(layout.height - px(4.));
    let y = (layout.height - tall) / 2;
    let baseline = y + ((tall as f32 + fonts.cap(value_size)) / 2.).round() as i32;
    let (pad, inside, dot, gap) = (px(8.), px(9.), px(7.), px(5.));
    let mut room = |key: String, cell: &Cell| {
        held(
            &mut reserve,
            key,
            figure_width(fonts, &cell.value, value_size, unit),
            figure_width(fonts, &cell.widest, value_size, unit),
        )
    };
    let mut x = pad;
    for group in groups {
        if group.cells.is_empty() {
            continue;
        }
        if !layout.pills.is_empty() {
            x += gap;
        }
        let start = x;
        x += inside;
        let dot_at = match group.cells.as_slice() {
            [cell] => {
                let centre = [x as f32 + dot as f32 / 2., y as f32 + tall as f32 / 2.];
                x += dot + px(5.);
                layout.runs.push(Run {
                    text: cell.tag().to_owned(),
                    x,
                    baseline,
                    size: name_size,
                    value: false,
                    paint: Paint::Ink,
                    shade: Shade::Name,
                });
                x += fonts.width(cell.tag(), name_size, false) + px(5.);
                // The value right after its name, as rates follow their
                // arrows: room held for a wider one lies after it.
                figure_from(
                    &mut layout.runs,
                    fonts,
                    &cell.value,
                    x,
                    baseline,
                    value_size,
                    unit,
                    value_paint(cell),
                );
                x += room(cell.kept("capsules"), cell);
                Some((centre, dot as f32 / 2.))
            }
            cells => {
                for (row, cell) in cells.iter().enumerate() {
                    if row > 0 {
                        x += px(8.);
                    }
                    layout.runs.push(Run {
                        text: cell.label.clone(),
                        x,
                        baseline,
                        size: name_size,
                        value: true,
                        paint: Paint::Tone(group.tone),
                        shade: Shade::Full,
                    });
                    x += fonts.width(&cell.label, name_size, true) + px(4.);
                    // The rate right after its arrow; room for a wider one
                    // follows it, so the next arrow keeps its place.
                    figure_from(
                        &mut layout.runs,
                        fonts,
                        &cell.value,
                        x,
                        baseline,
                        value_size,
                        unit,
                        value_paint(cell),
                    );
                    x += room(cell.kept("capsules"), cell);
                }
                None
            }
        };
        x += inside;
        // The dot marks the device, as a name does: in its colour even
        // when the value is amber.
        layout.pills.push(Pill {
            rect: [start, y, x, y + tall],
            dot: dot_at,
            paint: Paint::Tone(group.tone),
        });
        ended(ends, x, layout);
    }
    if layout.pills.is_empty() { 0 } else { x + pad }
}

/// Figures: a small name in its device's colour over a large figure, both
/// from the column's left edge; a pair of rates in two rows. Each column
/// keeps the room its values took lately, as capsules do. The layout's
/// width.
fn figures(
    layout: &mut Layout,
    ends: &mut Vec<End>,
    groups: &[Group],
    scale: f32,
    fonts: &impl Fonts,
    mut reserve: Option<(&mut Reserve, Instant)>,
) -> i32 {
    let px = |logical: f32| (logical * scale).round() as i32;
    let (name_size, large, unit, body) = (px(12.), px(20.), px(12.), px(14.));
    let middle = layout.height / 2;
    let (pad, gap) = (px(8.), px(9.));
    let mut x = pad;
    let mut any = false;
    for group in groups {
        if group.cells.is_empty() {
            continue;
        }
        if any {
            x += gap;
        }
        any = true;
        match group.cells.as_slice() {
            [cell] => {
                let room = held(
                    &mut reserve,
                    cell.kept("figures"),
                    figure_width(fonts, &cell.value, large, unit),
                    figure_width(fonts, &cell.widest, large, unit),
                );
                let width = fonts.width(cell.tag(), name_size, false).max(room);
                // In its device's colour even when the value is amber.
                layout.runs.push(Run {
                    text: cell.tag().to_owned(),
                    x,
                    baseline: middle - px(5.),
                    size: name_size,
                    value: false,
                    paint: Paint::Tone(group.tone),
                    shade: Shade::Full,
                });
                // The figure starts under its name: room left over for a
                // wider value lies after it, not between the columns' text.
                figure_from(
                    &mut layout.runs,
                    fonts,
                    &cell.value,
                    x,
                    middle + px(14.),
                    large,
                    unit,
                    value_paint(cell),
                );
                x += width;
            }
            cells => {
                let marks = cells
                    .iter()
                    .map(|cell| fonts.width(&cell.label, body, true))
                    .max()
                    .unwrap_or(0);
                let room = cells
                    .iter()
                    .map(|cell| {
                        held(
                            &mut reserve,
                            cell.kept("figures"),
                            figure_width(fonts, &cell.value, body, unit),
                            figure_width(fonts, &cell.widest, body, unit),
                        )
                    })
                    .max()
                    .unwrap_or(0);
                let width = marks + px(4.) + room;
                for (row, cell) in cells.iter().take(2).enumerate() {
                    let baseline = middle + [-px(3.), px(14.)][row];
                    layout.runs.push(Run {
                        text: cell.label.clone(),
                        x,
                        baseline,
                        size: body,
                        value: true,
                        paint: Paint::Tone(group.tone),
                        shade: Shade::Full,
                    });
                    figure(
                        &mut layout.runs,
                        fonts,
                        &cell.value,
                        x + width,
                        baseline,
                        body,
                        unit,
                        value_paint(cell),
                    );
                }
                x += width;
            }
        }
        ended(ends, x, layout);
    }
    if any { x + pad } else { 0 }
}

#[cfg(test)]
mod tests {
    use super::{
        Cell, Fonts, Group, Look, Paint, Run, Shade, Tone, columns, font_size, lay_out,
        lay_out_within, size_scale, split,
    };
    use argus_app::settings::{TaskbarSize, TaskbarStyle as Style};

    fn look(style: Style) -> Look {
        Look {
            style,
            ..Look::default()
        }
    }

    /// Every character 10 px wide whatever its size; capitals 9 px high at
    /// 13 px.
    struct Even;

    impl Fonts for Even {
        fn width(&self, text: &str, _size: i32, _value: bool) -> i32 {
            text.chars().count() as i32 * 10
        }

        fn cap(&self, size: i32) -> f32 {
            size as f32 * 9. / 13.
        }
    }

    fn cell(label: &str, value: &str) -> Cell {
        Cell {
            label: label.into(),
            value: value.into(),
            widest: "100%".into(),
            ..Cell::default()
        }
    }

    /// A reading of the device `run` names, after another of it.
    fn after(run: usize, label: &str, name: &str, value: &str) -> Group {
        let mut group = single(label, value);
        group.cells[0].name = name.into();
        group.cells[0].run = run;
        group
    }

    fn first(run: usize, label: &str, value: &str) -> Group {
        let mut group = single(label, value);
        group.cells[0].run = run;
        group
    }

    fn texts(runs: &[Run]) -> Vec<&str> {
        runs.iter().map(|run| run.text.as_str()).collect()
    }

    fn single(label: &str, value: &str) -> Group {
        Group {
            cells: vec![cell(label, value)],
            tone: Tone::Cpu,
            history: vec![0.2, 0.6, 0.4],
        }
    }

    fn rates(down: &str, up: &str) -> Group {
        let rate = |label: &str, value: &str| Cell {
            label: label.into(),
            value: value.into(),
            widest: "999.9 MB/s".into(),
            ..Cell::default()
        };
        Group {
            cells: vec![rate("↓", down), rate("↑", up)],
            tone: Tone::Network,
            history: vec![0.1, 0.3],
        }
    }

    fn names(columns: &[Vec<(&Cell, &Group)>]) -> Vec<Vec<String>> {
        columns
            .iter()
            .map(|column| column.iter().map(|(cell, _)| cell.label.clone()).collect())
            .collect()
    }

    fn values(runs: &[Run]) -> Vec<&Run> {
        runs.iter()
            .filter(|run| run.value && run.shade == Shade::Full)
            .collect()
    }

    #[test]
    fn only_the_first_readings_that_fit_the_room_show() {
        let groups = vec![
            single("CPU", "1%"),
            single("MEM", "3%"),
            rates("1 KB/s", "2 KB/s"),
        ];
        for style in [
            Style::Labels,
            Style::Curves,
            Style::Capsules,
            Style::Figures,
        ] {
            let whole = lay_out(&groups, 48, 1., look(style), &Even, None);
            // A pixel short: the rates go, with their tile or capsule, and
            // the padding stays.
            let short = lay_out_within(&groups, 48, 1., look(style), &Even, None, whole.width - 1);
            assert!(short.width > 0 && short.width < whole.width, "{style:?}");
            assert_eq!(short.runs, whole.runs[..short.runs.len()], "{style:?}");
            assert!(short.runs.iter().all(|run| run.text != "↓"), "{style:?}");
            assert!(short.runs.iter().any(|run| run.text == "MEM"), "{style:?}");
            assert_eq!(short.tiles.len(), whole.tiles.len().min(2), "{style:?}");
            assert_eq!(short.pills.len(), whole.pills.len().min(2), "{style:?}");
            // Its own width holds it all; too little for one, none.
            assert_eq!(
                lay_out_within(&groups, 48, 1., look(style), &Even, None, whole.width),
                whole
            );
            let none = lay_out_within(&groups, 48, 1., look(style), &Even, None, 20);
            assert!(none.width == 0 && none.runs.is_empty(), "{style:?}");
        }
        // Labels keep a column whole: CPU and MEM go together.
        let labels = lay_out(&groups, 48, 1., look(Style::Labels), &Even, None);
        let first = lay_out_within(
            &groups,
            48,
            1.,
            look(Style::Labels),
            &Even,
            None,
            8 + 77 + 8,
        );
        assert_eq!((first.width, first.runs.len()), (8 + 77 + 8, 4));
        assert!(labels.width > first.width);
    }

    #[test]
    fn single_readings_pair_up_and_rates_keep_their_own_column() {
        let groups = vec![
            single("CPU", "1%"),
            rates("1 KB/s", "2 KB/s"),
            single("MEM", "3%"),
            single("GPU", "4%"),
        ];
        assert_eq!(
            names(&columns(&groups, 2)),
            [vec!["CPU", "MEM"], vec!["↓", "↑"], vec!["GPU"]]
        );
        // One row: every reading in a column of its own, in order.
        assert_eq!(
            names(&columns(&groups, 1)),
            [vec!["CPU"], vec!["↓"], vec!["↑"], vec!["MEM"], vec!["GPU"]]
        );
        assert!(columns(&[], 2).is_empty());
    }

    #[test]
    fn labels_keep_the_width_of_their_widest_value_and_rows_centre() {
        let groups = vec![single("CPU", "1%"), single("MEM", "45%")];
        let layout = lay_out(&groups, 48, 1., look(Style::Labels), &Even, None);
        // Padding 8, name 30, gap 7, the widest value "100%" 40, padding 8.
        assert_eq!(layout.width, 8 + 30 + 7 + 40 + 8);
        assert_eq!(layout.font, 15);
        assert!(layout.tiles.is_empty());
        // Two rows of 20 px in the middle of 48, the capitals centred.
        let shown = values(&layout.runs);
        assert_eq!((shown[0].x, shown[0].baseline), (8 + 77 - 20, 4 + 15));
        assert_eq!((shown[1].x, shown[1].baseline), (8 + 77 - 30, 24 + 15));
        // The same readings with other values keep the size.
        let busier = vec![single("CPU", "100%"), single("MEM", "99%")];
        assert_eq!(
            lay_out(&busier, 48, 1., look(Style::Labels), &Even, None).width,
            layout.width
        );
        // On the taskbar a column keeps only the room its values took
        // lately: "45%" 30, not the widest "100%".
        let mut reserve = super::Reserve::default();
        let held = lay_out(
            &groups,
            48,
            1.,
            look(Style::Labels),
            &Even,
            Some((&mut reserve, std::time::Instant::now())),
        );
        assert_eq!(held.width, layout.width - 10);
        // A taskbar too low for two rows sets them side by side, everything
        // smaller: 13 px text, padding 7, gap 6, 10 between columns.
        let low = lay_out(&groups, 30, 1., look(Style::Labels), &Even, None);
        assert_eq!(low.font, 13);
        assert_eq!(low.width, 7 + 76 + 10 + 76 + 7);
        assert!(low.runs.iter().all(|run| run.baseline == 6 + 13));
        // Windows 10's 40 px taskbar still takes two rows of 13 px.
        let lower = lay_out(&groups, 40, 1., look(Style::Labels), &Even, None);
        assert_eq!(lower.font, 13);
        assert_eq!(
            values(&lower.runs)[1].baseline - values(&lower.runs)[0].baseline,
            17
        );
        // At 150 % everything grows with the scale, and a taller taskbar
        // grows it too, within bounds.
        assert_eq!(font_size(1.5), 23);
        assert_eq!(size_scale(72, 1.5), 1.5);
        assert_eq!(size_scale(56, 1.), 56. / 48.);
        assert_eq!(size_scale(100, 1.), 1.25);
    }

    #[test]
    fn labels_follow_their_row_size_and_colour_settings() {
        let mut uptime = single("Up", "5 d 14 h");
        uptime.tone = Tone::Plain;
        let groups = vec![single("CPU", "1%"), single("MEM", "45%")];
        let two = lay_out(&groups, 48, 1., look(Style::Labels), &Even, None);
        let one = lay_out(
            &groups,
            48,
            1.,
            Look {
                rows: 1,
                ..look(Style::Labels)
            },
            &Even,
            None,
        );
        // Two rows put the second reading under the first; one row sets
        // them side by side.
        assert_ne!(values(&two.runs)[0].baseline, values(&two.runs)[1].baseline);
        assert_eq!(values(&one.runs)[0].baseline, values(&one.runs)[1].baseline);
        // Values match the taskbar ink by default, or take their device's
        // colour when enabled; uptime has no device colour and stays ink,
        // and a high value stays amber either way.
        assert_eq!(values(&two.runs)[0].paint, Paint::Ink);
        let colored = lay_out(
            &[groups[0].clone(), uptime],
            48,
            1.,
            Look {
                colored: true,
                ..look(Style::Labels)
            },
            &Even,
            None,
        );
        assert_eq!(values(&colored.runs)[0].paint, Paint::Tone(Tone::Cpu));
        assert_eq!(values(&colored.runs)[1].paint, Paint::Ink);
        let mut high = groups.clone();
        high[0].cells[0].high = true;
        for colored in [false, true] {
            let amber = lay_out(
                &high,
                48,
                1.,
                Look {
                    colored,
                    ..look(Style::Labels)
                },
                &Even,
                None,
            );
            assert_eq!(values(&amber.runs)[0].paint, Paint::High);
        }
        // Large text keeps two rows on Windows 11's 48 px and Windows 10's
        // 40 px taskbars when two rows are chosen, and falls back to one row
        // on a low 30 px taskbar.
        for height in [48, 40] {
            let large = lay_out(
                &groups,
                height,
                1.,
                Look {
                    size: TaskbarSize::Large,
                    ..look(Style::Labels)
                },
                &Even,
                None,
            );
            let medium = lay_out(&groups, height, 1., look(Style::Labels), &Even, None);
            assert!(large.font > medium.font, "{height}");
            assert_ne!(
                values(&large.runs)[0].baseline,
                values(&large.runs)[1].baseline,
                "{height}"
            );
        }
        let large_low = lay_out(
            &groups,
            30,
            1.,
            Look {
                size: TaskbarSize::Large,
                ..look(Style::Labels)
            },
            &Even,
            None,
        );
        assert_eq!(
            values(&large_low.runs)[0].baseline,
            values(&large_low.runs)[1].baseline
        );
        let small = lay_out(
            &groups,
            48,
            1.,
            Look {
                size: TaskbarSize::Small,
                ..look(Style::Labels)
            },
            &Even,
            None,
        );
        assert!(small.font < two.font);
        // The tile styles grow with the size too.
        let tiles = lay_out(
            &groups,
            48,
            1.,
            Look {
                size: TaskbarSize::Large,
                ..look(Style::Curves)
            },
            &Even,
            None,
        );
        let medium_tiles = lay_out(&groups, 48, 1., look(Style::Curves), &Even, None);
        assert!(
            tiles.tiles[0].rect[3] - tiles.tiles[0].rect[1]
                > medium_tiles.tiles[0].rect[3] - medium_tiles.tiles[0].rect[1]
        );
    }

    #[test]
    fn curves_set_each_reading_on_a_tile_over_its_curve() {
        let groups = vec![single("CPU", "1%"), rates("1 KB/s", "2 KB/s")];
        let layout = lay_out(&groups, 48, 1., look(Style::Curves), &Even, None);
        assert_eq!(layout.tiles.len(), 2);
        // Name 30 and the widest figure "100" with its unit "%", 40, inside
        // 9 px either side and 9 apart; tiles 40 px high in the middle.
        assert_eq!(layout.tiles[0].rect, [8, 4, 8 + 97, 44]);
        assert_eq!(layout.tiles[0].curve, [9, 24, 104, 43]);
        assert_eq!(layout.tiles[0].paint, Paint::Tone(Tone::Cpu));
        assert_eq!(layout.tiles[0].history, [0.2, 0.6, 0.4]);
        // A pair of rates: its marks, then the widest "999.9 MB/s".
        assert_eq!(layout.tiles[1].rect, [111, 4, 111 + 134, 44]);
        assert_eq!(layout.tiles[1].curve[1], 4 + 29);
        assert_eq!(layout.width, 245 + 8);
        // The unit is smaller and quieter than its figure.
        let unit = layout.runs.iter().find(|run| run.text == "%").unwrap();
        assert_eq!((unit.size, unit.shade), (12, Shade::Unit));
        // Other values keep the tiles where they are; a high one turns the
        // tile's curve amber.
        let mut busier = vec![single("CPU", "100%"), rates("999.9 MB/s", "0 KB/s")];
        busier[0].cells[0].high = true;
        let again = lay_out(&busier, 48, 1., look(Style::Curves), &Even, None);
        assert_eq!(
            again.tiles.iter().map(|tile| tile.rect).collect::<Vec<_>>(),
            layout
                .tiles
                .iter()
                .map(|tile| tile.rect)
                .collect::<Vec<_>>()
        );
        assert_eq!(again.tiles[0].paint, Paint::High);
    }

    #[test]
    fn figures_set_a_large_figure_under_a_name_in_its_colour() {
        let layout = lay_out(
            &[single("CPU", "41%")],
            48,
            1.,
            look(Style::Figures),
            &Even,
            None,
        );
        let name = &layout.runs[0];
        assert_eq!((name.size, name.baseline), (12, 24 - 5));
        assert_eq!(name.paint, Paint::Tone(Tone::Cpu));
        let figure = values(&layout.runs)[0];
        assert_eq!(
            (figure.text.as_str(), figure.size, figure.baseline),
            ("41", 20, 24 + 14)
        );
        // The figure starts under its name.
        assert_eq!(figure.x, name.x);
        assert_eq!(layout.width, 8 + 40 + 8);
        // On the taskbar a column keeps only the room its values took
        // lately: "41%" 30, not the widest "100%".
        let mut reserve = super::Reserve::default();
        let now = std::time::Instant::now();
        let held = lay_out(
            &[single("CPU", "41%"), single("GPU", "7%")],
            48,
            1.,
            look(Style::Figures),
            &Even,
            Some((&mut reserve, now)),
        );
        // Padding 8, "41%" 30, gap 9, the name "GPU" 30 over "7%" 20,
        // padding 8.
        assert_eq!(held.width, 8 + 30 + 9 + 30 + 8);
        // A taskbar too low for two rows shows labels instead.
        let low = lay_out(
            &[single("CPU", "41%")],
            30,
            1.,
            look(Style::Figures),
            &Even,
            None,
        );
        assert!(low.runs.iter().all(|run| run.size == 13));
    }

    #[test]
    fn a_parameter_goes_by_its_name_where_its_device_shows() {
        // CPU, its temperature, the memory, then the GPU's temperature
        // after the GPU.
        let groups = vec![
            first(1, "CPU", "41%"),
            after(1, "CPU", "Temp", "59 °C"),
            first(3, "RAM", "40%"),
            first(4, "GPU", "94%"),
            after(4, "GPU", "Temp", "48 °C"),
        ];
        // In colour, each after its device's reading.
        for style in [Style::Figures, Style::Capsules, Style::Curves] {
            let layout = lay_out(&groups, 48, 1., look(style), &Even, None);
            assert_eq!(
                texts(&layout.runs)
                    .into_iter()
                    .filter(|text| text.chars().all(char::is_alphabetic))
                    .collect::<Vec<_>>(),
                ["CPU", "Temp", "RAM", "GPU", "Temp"],
                "{style:?}"
            );
        }
        // In labels, two rows: only under its device's reading; the GPU's
        // temperature opens a column of its own and names the GPU.
        let labels = lay_out(&groups, 48, 1., look(Style::Labels), &Even, None);
        let names = labels
            .runs
            .iter()
            .filter(|run| !run.value)
            .map(|run| run.text.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, ["CPU", "Temp", "RAM", "GPU", "GPU"]);
        // In one row, right after it.
        let low = lay_out(&groups, 30, 1., look(Style::Labels), &Even, None);
        let names = low
            .runs
            .iter()
            .filter(|run| !run.value)
            .map(|run| run.text.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, ["CPU", "Temp", "RAM", "GPU", "Temp"]);
    }

    #[test]
    fn a_high_value_turns_amber_and_its_name_keeps_its_colour() {
        let mut busy = single("GPU", "94%");
        busy.tone = Tone::Gpu;
        busy.cells[0].high = true;
        let figures = lay_out(&[busy.clone()], 48, 1., look(Style::Figures), &Even, None);
        assert_eq!(figures.runs[0].paint, Paint::Tone(Tone::Gpu));
        assert!(
            values(&figures.runs)
                .iter()
                .all(|run| run.paint == Paint::High)
        );
        let capsules = lay_out(&[busy.clone()], 48, 1., look(Style::Capsules), &Even, None);
        assert_eq!(capsules.pills[0].paint, Paint::Tone(Tone::Gpu));
        assert!(
            values(&capsules.runs)
                .iter()
                .all(|run| run.paint == Paint::High)
        );
        // Labels name it in its colour too, as strongly as figures do.
        let labels = lay_out(&[busy], 48, 1., look(Style::Labels), &Even, None);
        assert_eq!(
            (labels.runs[0].paint, labels.runs[0].shade),
            (Paint::Tone(Tone::Gpu), Shade::Full)
        );
        assert!(
            values(&labels.runs)
                .iter()
                .all(|run| run.paint == Paint::High)
        );
    }

    #[test]
    fn labels_name_a_reading_in_its_colour_and_one_of_no_device_plainly() {
        let mut uptime = single("Up", "5 d 14 h");
        uptime.tone = Tone::Plain;
        let groups = vec![single("CPU", "41%"), rates("1 KB/s", "2 KB/s"), uptime];
        // Two rows, and one on a low taskbar.
        for height in [48, 30] {
            let layout = lay_out(&groups, height, 1., look(Style::Labels), &Even, None);
            let mut names = layout
                .runs
                .iter()
                .filter(|run| !run.value)
                .map(|run| (run.text.as_str(), run.paint, run.shade))
                .collect::<Vec<_>>();
            names.sort_by_key(|(text, ..)| *text);
            assert_eq!(
                names,
                [
                    ("CPU", Paint::Tone(Tone::Cpu), Shade::Full),
                    ("Up", Paint::Ink, Shade::Name),
                    ("↑", Paint::Tone(Tone::Network), Shade::Full),
                    ("↓", Paint::Tone(Tone::Network), Shade::Full),
                ],
                "{height}"
            );
            // The values keep the text's own colour by default.
            assert!(
                values(&layout.runs)
                    .iter()
                    .all(|run| run.paint == Paint::Ink)
            );
        }
    }

    #[test]
    fn capsules_line_up_in_one_row_and_keep_their_width() {
        let groups = vec![single("CPU", "1%"), rates("1 KB/s", "2 KB/s")];
        let layout = lay_out(&groups, 48, 1., look(Style::Capsules), &Even, None);
        assert_eq!(layout.pills.len(), 2);
        // 30 px high in the middle of 48. Inside 9, a dot of 7 and 5, the
        // name 30 and 5, the widest "100%" 40, inside 9: 105.
        assert_eq!(layout.pills[0].rect, [8, 9, 8 + 105, 39]);
        assert_eq!(layout.pills[0].dot, Some(([20.5, 24.], 3.5)));
        assert_eq!(layout.pills[0].paint, Paint::Tone(Tone::Cpu));
        let (name, value) = (&layout.runs[0], values(&layout.runs)[0]);
        assert_eq!(value.x, name.x + 30 + 5);
        // Rates share one: each arrow 10 and 4 before the widest
        // "999.9 MB/s" 100, 8 between them.
        assert_eq!(
            layout.pills[1].rect,
            [118, 9, 118 + 9 + 114 + 8 + 114 + 9, 39]
        );
        assert!(layout.pills[1].dot.is_none());
        // Other values keep the capsules where they are.
        let busier = vec![single("CPU", "100%"), rates("999.9 MB/s", "0 KB/s")];
        assert_eq!(
            lay_out(&busier, 48, 1., look(Style::Capsules), &Even, None).pills,
            layout.pills
        );
    }

    #[test]
    fn a_reading_keeps_its_own_room_when_another_takes_its_place() {
        use super::Reserve;
        use std::time::{Duration, Instant};
        let reading = |key: &str, value: &str| {
            let mut group = single("RAM", value);
            group.cells[0].key = key.into();
            group
        };
        let (share, used) = (reading("memory", "73%"), reading("memoryUsed", "23.3 GB"));
        let start = Instant::now();
        let later = start + Duration::from_secs(1);
        for style in [Style::Figures, Style::Capsules, Style::Labels] {
            // The amount in use alone, then its share added before it: the
            // share, where the amount was, keeps only its own room.
            let mut reserve = Reserve::default();
            lay_out(
                std::slice::from_ref(&used),
                48,
                1.,
                look(style),
                &Even,
                Some((&mut reserve, start)),
            );
            let both = [share.clone(), used.clone()];
            let after = lay_out(
                &both,
                48,
                1.,
                look(style),
                &Even,
                Some((&mut reserve, later)),
            );
            let fresh = lay_out(
                &both,
                48,
                1.,
                look(style),
                &Even,
                Some((&mut Reserve::default(), later)),
            );
            assert_eq!(after.width, fresh.width, "{style:?}");
        }
    }

    #[test]
    fn capsules_keep_the_room_their_values_took_within_a_minute() {
        use super::Reserve;
        use std::time::{Duration, Instant};
        let mut reserve = Reserve::default();
        let start = Instant::now();
        let mut width = |value: &str, seconds: u64| {
            let layout = lay_out(
                &[single("CPU", value)],
                48,
                1.,
                look(Style::Capsules),
                &Even,
                Some((&mut reserve, start + Duration::from_secs(seconds))),
            );
            layout.pills[0].rect[2] - layout.pills[0].rect[0]
        };
        // Only as wide as the value: inside 9, dot 7 and 5, name 30 and 5,
        // "1%" 20, inside 9.
        let narrow = width("1%", 0);
        assert_eq!(narrow, 85);
        // A wider value widens it at once…
        assert_eq!(width("100%", 5), narrow + 20);
        // …and its room holds for a minute after it.
        assert_eq!(width("1%", 30), narrow + 20);
        assert_eq!(width("1%", 64), narrow + 20);
        assert_eq!(width("1%", 66), narrow);
        // Changing the text size drops widths measured for the old size at
        // once rather than holding them for a minute.
        lay_out(
            &[single("CPU", "100%")],
            48,
            1.,
            Look {
                size: TaskbarSize::Large,
                ..look(Style::Capsules)
            },
            &Even,
            Some((&mut reserve, start + Duration::from_secs(70))),
        );
        let small = lay_out(
            &[single("CPU", "1%")],
            48,
            1.,
            Look {
                size: TaskbarSize::Small,
                ..look(Style::Capsules)
            },
            &Even,
            Some((&mut reserve, start + Duration::from_secs(71))),
        );
        let fresh_small = lay_out(
            &[single("CPU", "1%")],
            48,
            1.,
            Look {
                size: TaskbarSize::Small,
                ..look(Style::Capsules)
            },
            &Even,
            Some((&mut Reserve::default(), start + Duration::from_secs(71))),
        );
        assert_eq!(small.width, fresh_small.width);
    }

    #[test]
    fn units_part_from_their_figures() {
        assert_eq!(split("12.4 MB/s"), ("12.4", " MB/s"));
        assert_eq!(split("40% TDP"), ("40", "% TDP"));
        assert_eq!(split("—"), ("—", ""));
        // An uptime stays whole.
        assert_eq!(split("5 d 14 h"), ("5 d 14 h", ""));
    }
}
