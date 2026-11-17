//! What the taskbar monitor shows, drawn into premultiplied BGRA for a
//! layered window, in one of three styles (`TaskbarStyle`):
//!
//! - labels: short readings in columns of two rows (one on a taskbar too low
//!   for two), each a name on the left and its value on the right;
//! - curves: a tile per reading, its name and value over its recent curve in
//!   its device's colour;
//! - capsules: a rounded capsule per reading in one row, a dot in its
//!   device's colour before its name and value;
//! - figures: a small name in its device's colour over a large figure.
//!
//! Every value keeps the room of the widest it may get (`Cell::widest`), so
//! that changing values neither move the readings nor resize the window.
//! Curves, capsules and figures need two rows of text; a lower taskbar
//! shows labels.
//!
//! The text is set as the interface sets its own (`gpui-pre-windows`
//! `direct_write.rs`): DirectWrite lays it out in the interface's face, with
//! tabular figures and the interface language's fallback for other scripts;
//! each glyph is sampled three times per pixel across, the samples averaged
//! into one coverage, and the coverage corrected for the display's gamma
//! and contrast before it tints the pixel. Grey-scale GDI text, which this
//! replaced, had few levels of coverage, hinted curves into steps and left
//! the coverage uncorrected: light text on a dark taskbar looked ragged and
//! soft.
use crate::settings::TaskbarStyle as Style;
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};
use windows::Win32::Graphics::DirectWrite::*;
use windows::core::{BOOL, HSTRING, IUnknown, Interface, Ref, implement};

/// One reading on the taskbar.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Cell {
    /// Its short name: "CPU", or an arrow for a rate.
    pub label: String,
    pub value: String,
    /// The widest value it takes, so that it keeps its room.
    pub widest: String,
    /// High enough to show amber.
    pub high: bool,
}

/// The device a reading is of, for its colour.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Tone {
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
pub(crate) struct Group {
    pub cells: Vec<Cell>,
    pub tone: Tone,
    pub history: Vec<f32>,
}

/// Arranges the groups in columns of `rows` cells: a group's cells stay
/// together, and single cells share a column in order.
pub(crate) fn columns(groups: &[Group], rows: usize) -> Vec<Vec<&Cell>> {
    stacks(groups, rows)
        .into_iter()
        .map(|column| column.into_iter().map(|(cell, _)| cell).collect())
        .collect()
}

/// `columns`, each cell with the group it is of.
fn stacks(groups: &[Group], rows: usize) -> Vec<Vec<(&Cell, &Group)>> {
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
pub(crate) trait Fonts {
    /// The width of `text` set `size` pixels high, in a value's weight or a
    /// name's.
    fn width(&self, text: &str, size: i32, value: bool) -> i32;
    /// The capitals' height at `size`.
    fn cap(&self, size: i32) -> f32;
}

/// The colour of a run: the text's own, amber, or its device's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Paint {
    Ink,
    High,
    Tone(Tone),
}

/// How strongly a run shows: a value in full, a name quieter, a unit
/// quieter still.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Shade {
    Full,
    Name,
    Unit,
}

/// Where a text goes, in pixels of the image.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Run {
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
pub(crate) struct Tile {
    pub rect: [i32; 4],
    /// Where its curve goes.
    pub curve: [i32; 4],
    pub history: Vec<f32>,
    pub paint: Paint,
}

/// A capsule behind a reading, and the dot that marks its device.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Pill {
    pub rect: [i32; 4],
    /// The dot's centre and radius, none for a rate (its arrow marks it).
    pub dot: Option<([f32; 2], f32)>,
    pub paint: Paint,
}

/// The monitor laid out at one size.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Layout {
    pub width: i32,
    pub height: i32,
    pub runs: Vec<Run>,
    pub tiles: Vec<Tile>,
    pub pills: Vec<Pill>,
    /// The labels' text size, pixels.
    pub font: i32,
}

/// The labels' text size at `scale`, pixels: the interface's own
/// (`resources/theme.json` `fontSize`). Two rows of it fit a 48 px taskbar;
/// at the clock's 12 px the readings were harder to make out.
pub(crate) fn font_size(scale: f32) -> i32 {
    (13. * scale).round().max(8.) as i32
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

/// How wide each capsule's values have been lately: a capsule keeps the
/// room its value took within the last minute, widening at once for a wider
/// value and narrowing a minute after it, rather than the room of the
/// widest value it may ever take.
#[derive(Default)]
pub(crate) struct Reserve {
    widths: HashMap<String, VecDeque<(Instant, i32)>>,
}

/// How long a value's width holds its room.
const KEEP: Duration = Duration::from_secs(60);

impl Reserve {
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

/// Lays out `groups` in `style` in a window `height` pixels high at
/// `scale`; capsules keep their recent room in `reserve` when given one,
/// at the time given with it, and the room of their widest values without.
pub(crate) fn lay_out(
    groups: &[Group],
    height: i32,
    scale: f32,
    style: Style,
    fonts: &impl Fonts,
    reserve: Option<(&mut Reserve, Instant)>,
) -> Layout {
    let px = |logical: f32| (logical * scale).round() as i32;
    let style = if height < px(36.) {
        Style::Labels
    } else {
        style
    };
    let mut layout = Layout {
        height,
        font: font_size(scale),
        ..Layout::default()
    };
    layout.width = match style {
        Style::Labels => labels(&mut layout, groups, scale, fonts),
        Style::Curves => curves(&mut layout, groups, scale, fonts),
        Style::Capsules => capsules(&mut layout, groups, scale, fonts, reserve),
        Style::Figures => figures(&mut layout, groups, scale, fonts),
    };
    layout
}

/// Labels: names and values in columns of two rows; the layout's width.
fn labels(layout: &mut Layout, groups: &[Group], scale: f32, fonts: &impl Fonts) -> i32 {
    let px = |logical: f32| (logical * scale).round() as i32;
    let (font, height) = (layout.font, layout.height);
    let line = (font as f32 * 4. / 3.).round() as i32;
    let rows = if height >= 2 * line + px(4.) { 2 } else { 1 };
    // The capitals' middle on the row's middle.
    let baseline = ((line as f32 + fonts.cap(font)) / 2.).round() as i32;
    let (pad, inner, gap) = (px(8.), px(6.), px(14.));
    let mut x = pad;
    let columns = columns(groups, rows);
    for (index, column) in columns.iter().enumerate() {
        if index > 0 {
            x += gap;
        }
        let names = column
            .iter()
            .map(|cell| fonts.width(&cell.label, font, false))
            .collect::<Vec<_>>();
        let values = column
            .iter()
            .map(|cell| fonts.width(&cell.value, font, true))
            .collect::<Vec<_>>();
        let width = column
            .iter()
            .zip(&names)
            .zip(&values)
            .map(|((cell, name), value)| {
                let room = (*value).max(fonts.width(&cell.widest, font, true));
                if cell.label.is_empty() {
                    room
                } else {
                    name + inner + room
                }
            })
            .max()
            .unwrap_or(0);
        // A lone cell sits in the middle of the column's height.
        let top = (height - column.len() as i32 * line) / 2;
        for (row, ((cell, _), value)) in column.iter().zip(&names).zip(&values).enumerate() {
            let y = top + row as i32 * line + baseline;
            if !cell.label.is_empty() {
                layout.runs.push(Run {
                    text: cell.label.clone(),
                    x,
                    baseline: y,
                    size: font,
                    value: false,
                    paint: Paint::Ink,
                    shade: Shade::Name,
                });
            }
            layout.runs.push(Run {
                text: cell.value.clone(),
                x: x + width - value,
                baseline: y,
                size: font,
                value: true,
                paint: value_paint(cell),
                shade: Shade::Full,
            });
        }
        x += width;
    }
    if columns.is_empty() { 0 } else { x + pad }
}

/// Curves: a tile per reading, its name and value over its recent curve; a
/// pair of rates in two rows over theirs. The layout's width.
fn curves(layout: &mut Layout, groups: &[Group], scale: f32, fonts: &impl Fonts) -> i32 {
    let px = |logical: f32| (logical * scale).round() as i32;
    let (small, body) = (px(11.), px(12.));
    let tall = px(38.).min(layout.height);
    let top = (layout.height - tall) / 2;
    let bottom = top + tall;
    let (pad, inside, gap) = (px(8.), px(8.), px(6.));
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
                let name = fonts.width(&cell.label, small, false);
                let room = figure_width(fonts, &cell.widest, body, small).max(figure_width(
                    fonts,
                    &cell.value,
                    body,
                    small,
                ));
                let width = inside + name + px(8.) + room + inside;
                let baseline = top + px(13.);
                layout.runs.push(Run {
                    text: cell.label.clone(),
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
                (width, top + px(18.))
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
                    let baseline = top + [px(11.), px(23.)][row];
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
                (width, top + px(26.))
            }
        };
        layout.tiles.push(Tile {
            rect: [x, top, x + width, bottom],
            curve: [x + 1, curve, x + width - 1, bottom - 1],
            history: group.history.clone(),
            paint,
        });
        x += width;
    }
    if layout.tiles.is_empty() { 0 } else { x + pad }
}

/// Capsules: a rounded capsule per reading, side by side: a dot in its
/// device's colour, its name and its value right after it; a pair of rates
/// share one, each right after its arrow in its device's colour. The
/// layout's width.
fn capsules(
    layout: &mut Layout,
    groups: &[Group],
    scale: f32,
    fonts: &impl Fonts,
    mut reserve: Option<(&mut Reserve, Instant)>,
) -> i32 {
    let px = |logical: f32| (logical * scale).round() as i32;
    let (name_size, value_size, unit) = (px(11.), px(12.), px(10.));
    let tall = px(24.).min(layout.height - px(4.));
    let y = (layout.height - tall) / 2;
    let baseline = y + ((tall as f32 + fonts.cap(value_size)) / 2.).round() as i32;
    let (pad, inside, dot, gap) = (px(8.), px(7.), px(6.), px(4.));
    let mut room = |key: String, cell: &Cell| {
        let width = figure_width(fonts, &cell.value, value_size, unit);
        match reserve.as_mut() {
            Some((reserve, now)) => reserve.room(key, width, *now),
            None => width.max(figure_width(fonts, &cell.widest, value_size, unit)),
        }
    };
    let mut x = pad;
    for (index, group) in groups.iter().enumerate() {
        if group.cells.is_empty() {
            continue;
        }
        if !layout.pills.is_empty() {
            x += gap;
        }
        let high = group.cells.iter().any(|cell| cell.high);
        let start = x;
        x += inside;
        let dot_at = match group.cells.as_slice() {
            [cell] => {
                let centre = [x as f32 + dot as f32 / 2., y as f32 + tall as f32 / 2.];
                x += dot + px(4.);
                layout.runs.push(Run {
                    text: cell.label.clone(),
                    x,
                    baseline,
                    size: name_size,
                    value: false,
                    paint: Paint::Ink,
                    shade: Shade::Name,
                });
                x += fonts.width(&cell.label, name_size, false) + px(4.);
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
                x += room(format!("{index}:0:{}", cell.label), cell);
                Some((centre, dot as f32 / 2.))
            }
            cells => {
                for (row, cell) in cells.iter().enumerate() {
                    if row > 0 {
                        x += px(7.);
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
                    x += fonts.width(&cell.label, name_size, true) + px(3.);
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
                    x += room(format!("{index}:{row}:{}", cell.label), cell);
                }
                None
            }
        };
        x += inside;
        layout.pills.push(Pill {
            rect: [start, y, x, y + tall],
            dot: dot_at,
            paint: if high {
                Paint::High
            } else {
                Paint::Tone(group.tone)
            },
        });
    }
    if layout.pills.is_empty() { 0 } else { x + pad }
}

/// Figures: a small name in its device's colour over a large figure, both
/// from the column's left edge; a pair of rates in two rows. The layout's
/// width.
fn figures(layout: &mut Layout, groups: &[Group], scale: f32, fonts: &impl Fonts) -> i32 {
    let px = |logical: f32| (logical * scale).round() as i32;
    let (name_size, large, unit, body) = (px(10.), px(16.), px(10.), px(11.));
    let middle = layout.height / 2;
    let (pad, gap) = (px(8.), px(8.));
    let mut x = pad;
    let mut any = false;
    for group in groups.iter().filter(|group| !group.cells.is_empty()) {
        if any {
            x += gap;
        }
        any = true;
        match group.cells.as_slice() {
            [cell] => {
                let room = figure_width(fonts, &cell.widest, large, unit).max(figure_width(
                    fonts,
                    &cell.value,
                    large,
                    unit,
                ));
                let width = fonts.width(&cell.label, name_size, false).max(room);
                layout.runs.push(Run {
                    text: cell.label.clone(),
                    x,
                    baseline: middle - px(7.),
                    size: name_size,
                    value: false,
                    paint: if cell.high {
                        Paint::High
                    } else {
                        Paint::Tone(group.tone)
                    },
                    shade: Shade::Full,
                });
                // The figure starts under its name: room left over for a
                // wider value lies after it, not between the columns' text.
                figure_from(
                    &mut layout.runs,
                    fonts,
                    &cell.value,
                    x,
                    middle + px(13.),
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
                        figure_width(fonts, &cell.widest, body, unit).max(figure_width(
                            fonts,
                            &cell.value,
                            body,
                            unit,
                        ))
                    })
                    .max()
                    .unwrap_or(0);
                let width = marks + px(4.) + room;
                for (row, cell) in cells.iter().take(2).enumerate() {
                    let baseline = middle + [-px(4.), px(13.)][row];
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
    }
    if any { x + pad } else { 0 }
}

type Rgb = [f32; 3];

fn hex(value: u32) -> Rgb {
    [
        ((value >> 16) & 255) as f32 / 255.,
        ((value >> 8) & 255) as f32 / 255.,
        (value & 255) as f32 / 255.,
    ]
}

/// The taskbar's colours: text as the system's own (`tray_icon`), names a
/// step quieter than values and units quieter still, amber for high
/// values, the fill a taskbar button shows under the pointer, a tile's
/// fill, and the devices' colours as the interface's
/// (`resources/theme.json`).
struct Colors {
    ink: Rgb,
    high: Rgb,
    name: f32,
    unit: f32,
    hover: (Rgb, f32),
    tile: f32,
    /// A capsule's fill.
    pill: f32,
    /// A curve's fill and line.
    curve: (f32, f32),
    light: bool,
}

fn colors(light: bool) -> Colors {
    if light {
        Colors {
            ink: hex(0x1a1a1a),
            high: hex(0xc27a00),
            name: 0.72,
            unit: 0.6,
            hover: (hex(0x000000), 0.05),
            tile: 0.045,
            pill: 0.06,
            curve: (0.16, 0.8),
            light,
        }
    } else {
        Colors {
            ink: hex(0xffffff),
            high: hex(0xf4b446),
            name: 0.78,
            unit: 0.6,
            hover: (hex(0xffffff), 0.08),
            tile: 0.06,
            pill: 0.085,
            curve: (0.22, 0.9),
            light,
        }
    }
}

impl Colors {
    fn tone(&self, tone: Tone) -> Rgb {
        let [dark, light] = match tone {
            Tone::Plain => return self.ink,
            Tone::Cpu => [0x46cff4, 0x0a8bbd],
            Tone::Memory => [0x9b8cff, 0x6a52ec],
            Tone::Gpu => [0xff7aa8, 0xd6336c],
            Tone::Disk => [0xf4b446, 0xc27a00],
            Tone::Network => [0x4fd18b, 0x1f9d55],
            Tone::Battery => [0xa3d65c, 0x4f8a10],
        };
        hex(if self.light { light } else { dark })
    }

    fn paint(&self, paint: Paint) -> Rgb {
        match paint {
            Paint::Ink => self.ink,
            Paint::High => self.high,
            Paint::Tone(tone) => self.tone(tone),
        }
    }

    fn shade(&self, shade: Shade) -> f32 {
        match shade {
            Shade::Full => 1.,
            Shade::Name => self.name,
            Shade::Unit => self.unit,
        }
    }
}

/// How much of the pixel at (x, y) a rounded rectangle covers.
fn rounded(x: f32, y: f32, rect: [f32; 4], radius: f32) -> f32 {
    let [left, top, right, bottom] = rect;
    let (hw, hh) = ((right - left) / 2., (bottom - top) / 2.);
    let radius = radius.min(hw).min(hh).max(0.);
    let qx = (x - (left + hw)).abs() - (hw - radius);
    let qy = (y - (top + hh)).abs() - (hh - radius);
    let outside = (qx.max(0.).powi(2) + qy.max(0.).powi(2)).sqrt();
    let distance = outside + qx.max(qy).min(0.) - radius;
    (0.5 - distance).clamp(0., 1.)
}

/// Fills `rect` of premultiplied `pixels`, `width` wide, rounded.
fn fill(pixels: &mut [[f32; 4]], width: i32, rect: [f32; 4], radius: f32, color: Rgb, alpha: f32) {
    let height = pixels.len() as i32 / width.max(1);
    let [left, top, right, bottom] = rect;
    for y in (top.floor() as i32).max(0)..(bottom.ceil() as i32).min(height) {
        for x in (left.floor() as i32).max(0)..(right.ceil() as i32).min(width) {
            let a = alpha * rounded(x as f32 + 0.5, y as f32 + 0.5, rect, radius);
            blend(&mut pixels[(y * width + x) as usize], color, a);
        }
    }
}

/// Draws `values` (0–1, oldest first) as a curve across `rect` of
/// premultiplied `pixels`, `width` wide, the area under it filled.
fn sparkline(
    pixels: &mut [[f32; 4]],
    width: i32,
    rect: [i32; 4],
    values: &[f32],
    color: Rgb,
    (fill, line): (f32, f32),
) {
    let height = pixels.len() as i32 / width.max(1);
    let [left, top, right, bottom] = rect.map(|v| v as f32);
    let count = values.len();
    if count < 2 || right - left < 2. || bottom - top < 2. {
        return;
    }
    let at = |x: f32| {
        let f = ((x - left) / (right - left)).clamp(0., 1.) * (count - 1) as f32;
        let i = (f.floor() as usize).min(count - 1);
        let j = (i + 1).min(count - 1);
        let v = values[i] + (values[j] - values[i]) * (f - i as f32);
        bottom - (bottom - top) * v.clamp(0., 1.)
    };
    for x in (rect[0]).max(0)..(rect[2]).min(width) {
        let cx = x as f32 + 0.5;
        let curve = at(cx);
        let slope = at(cx + 0.5) - at(cx - 0.5);
        for y in (rect[1]).max(0)..(rect[3]).min(height) {
            let cy = y as f32 + 0.5;
            let pixel = &mut pixels[(y * width + x) as usize];
            blend(pixel, color, fill * (cy - curve + 0.5).clamp(0., 1.));
            let distance = (cy - curve).abs() / (1. + slope * slope).sqrt();
            blend(pixel, color, line * (1.15 - distance).clamp(0., 1.));
        }
    }
}

/// The weight of names and of values: values as the interface's emphasis
/// (Medium, which Segoe UI Variable draws at exactly 500 on its weight axis);
/// heavier weights fill in the digits at taskbar sizes.
const NAME_WEIGHT: f32 = 400.;
const VALUE_WEIGHT: f32 = 500.;

/// The faces tried in turn, as the interface's (`resources/theme.json`
/// `fonts.sans`); Chinese, Japanese and other scripts fall back by the
/// interface language.
const FAMILIES: [&str; 2] = ["Segoe UI Variable Text", "Segoe UI"];

/// Sets text with DirectWrite and composes the image: premultiplied BGRA,
/// `width` × `height`. The factory, the face, the text formats of each size
/// and the display's text rendering parameters stay for the next image.
pub(crate) struct Painter {
    factory: IDWriteFactory3,
    renderer: IDWriteTextRenderer,
    collection: IDWriteFontCollection1,
    family: HSTRING,
    locale: HSTRING,
    /// The formats made so far, by size and weight (a value's or a name's).
    formats: RefCell<HashMap<(i32, bool), IDWriteTextFormat>>,
    typography: IDWriteTypography,
    /// The capitals' height per pixel of size.
    cap: f32,
    value_weight: f32,
    /// The display's alpha correction, as the interface applies it.
    gamma: [f32; 4],
    contrast: f32,
    /// How the three samples of a pixel make one coverage.
    weights: [u16; 3],
    /// The labels' size it was made for.
    pub font: i32,
}

impl Painter {
    pub fn new(font: i32) -> Option<Self> {
        Self::with_weight(font, VALUE_WEIGHT)
    }

    fn with_weight(font: i32, value_weight: f32) -> Option<Self> {
        unsafe {
            let factory: IDWriteFactory3 = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).ok()?;
            let params: IDWriteRenderingParams1 =
                factory.CreateRenderingParams().ok()?.cast().ok()?;
            let weights = match params.GetPixelGeometry() {
                DWRITE_PIXEL_GEOMETRY_RGB => [54, 183, 19],
                DWRITE_PIXEL_GEOMETRY_BGR => [19, 183, 54],
                _ => [37, 182, 37],
            };
            let mut collection = None;
            factory
                .GetSystemFontCollection(false, &mut collection, false)
                .ok()?;
            let collection = collection?;
            let (family, index) = FAMILIES.iter().find_map(|name| {
                let (mut index, mut exists) = (0, BOOL(0));
                collection
                    .FindFamilyName(&HSTRING::from(*name), &mut index, &mut exists)
                    .ok()?;
                exists.as_bool().then(|| (HSTRING::from(*name), index))
            })?;
            let typography = factory.CreateTypography().ok()?;
            typography
                .AddFontFeature(DWRITE_FONT_FEATURE {
                    nameTag: DWRITE_FONT_FEATURE_TAG_TABULAR_FIGURES,
                    parameter: 1,
                })
                .ok()?;
            let face = collection
                .GetFontFamily(index)
                .ok()?
                .GetFirstMatchingFont(
                    DWRITE_FONT_WEIGHT_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    DWRITE_FONT_STYLE_NORMAL,
                )
                .ok()?;
            let mut metrics = DWRITE_FONT_METRICS::default();
            face.GetMetrics(&mut metrics);
            Some(Self {
                renderer: Renderer.into(),
                collection,
                family,
                locale: HSTRING::from(argus_core::i18n::locale().tag()),
                formats: RefCell::new(HashMap::new()),
                typography,
                cap: f32::from(metrics.capHeight) / f32::from(metrics.designUnitsPerEm.max(1)),
                value_weight,
                gamma: gpui_kit::gpui::get_gamma_correction_ratios(params.GetGamma()),
                contrast: params.GetGrayscaleEnhancedContrast(),
                weights,
                factory,
                font,
            })
        }
    }

    /// The format for text `size` pixels high, in a value's weight or a
    /// name's.
    fn format(&self, size: i32, value: bool) -> Option<IDWriteTextFormat> {
        if let Some(format) = self.formats.borrow().get(&(size, value)) {
            return Some(format.clone());
        }
        let weight = if value {
            self.value_weight
        } else {
            NAME_WEIGHT
        };
        let format = unsafe {
            let format = self
                .factory
                .CreateTextFormat(
                    &self.family,
                    &self.collection,
                    DWRITE_FONT_WEIGHT(weight as i32),
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    size.max(1) as f32,
                    &self.locale,
                )
                .ok()?;
            let _ = format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
            // A variable face is set at exactly this weight, as the
            // interface sets it, not at its nearest named instance.
            if let Ok(format) = format.cast::<IDWriteTextFormat3>() {
                let _ = format.SetFontAxisValues(&[DWRITE_FONT_AXIS_VALUE {
                    axisTag: DWRITE_FONT_AXIS_TAG_WEIGHT,
                    value: weight,
                }]);
            }
            format
        };
        self.formats
            .borrow_mut()
            .insert((size, value), format.clone());
        Some(format)
    }

    /// `text` laid out on one line.
    fn text(&self, text: &str, size: i32, value: bool) -> Option<IDWriteTextLayout> {
        let wide: Vec<u16> = text.encode_utf16().collect();
        let format = self.format(size, value)?;
        unsafe {
            let layout = self
                .factory
                .CreateTextLayout(&wide, &format, f32::INFINITY, f32::INFINITY)
                .ok()?;
            let _ = layout.SetTypography(
                &self.typography,
                DWRITE_TEXT_RANGE {
                    startPosition: 0,
                    length: wide.len() as u32,
                },
            );
            Some(layout)
        }
    }

    /// Draws `text` into premultiplied `pixels`, `width` × `height`, its
    /// baseline starting at `origin`.
    #[allow(clippy::too_many_arguments)]
    fn draw(
        &self,
        pixels: &mut [[f32; 4]],
        width: i32,
        height: i32,
        text: &str,
        size: i32,
        value: bool,
        origin: [f32; 2],
        color: Rgb,
        alpha: f32,
    ) {
        let Some(layout) = self.text(text, size, value) else {
            return;
        };
        let mut target = Target {
            painter: self,
            pixels,
            width,
            height,
            baseline: origin[1],
            color,
            alpha,
        };
        unsafe {
            let _ = layout.Draw(
                Some((&raw mut target).cast::<std::ffi::c_void>().cast_const()),
                &self.renderer,
                origin[0],
                origin[1],
            );
        }
    }

    /// The image of `layout` on a light or dark taskbar, with the pointer's
    /// fill when `hover`. Every pixel keeps a trace of alpha so that the
    /// whole window answers the pointer, not only its glyphs.
    pub fn paint(&self, layout: &Layout, light: bool, hover: bool, scale: f32) -> Option<Vec<u8>> {
        let (width, height) = (layout.width, layout.height);
        if width <= 0 || height <= 0 {
            return None;
        }
        let colors = colors(light);
        let mut pixels = vec![[0f32; 4]; (width * height) as usize];
        for pixel in &mut pixels {
            pixel[3] = 1. / 255.;
        }
        if hover {
            let inset = (2. * scale).round();
            let (color, alpha) = colors.hover;
            fill(
                &mut pixels,
                width,
                [0., inset, width as f32, height as f32 - inset],
                4. * scale,
                color,
                alpha,
            );
        }
        for tile in &layout.tiles {
            fill(
                &mut pixels,
                width,
                tile.rect.map(|v| v as f32),
                6. * scale,
                colors.ink,
                colors.tile,
            );
            sparkline(
                &mut pixels,
                width,
                tile.curve,
                &tile.history,
                colors.paint(tile.paint),
                colors.curve,
            );
        }
        for pill in &layout.pills {
            let [left, top, right, bottom] = pill.rect.map(|v| v as f32);
            fill(
                &mut pixels,
                width,
                [left, top, right, bottom],
                (bottom - top) / 2.,
                colors.ink,
                colors.pill,
            );
            if let Some(([x, y], radius)) = pill.dot {
                fill(
                    &mut pixels,
                    width,
                    [x - radius, y - radius, x + radius, y + radius],
                    radius,
                    colors.paint(pill.paint),
                    1.,
                );
            }
        }
        for run in &layout.runs {
            self.draw(
                &mut pixels,
                width,
                height,
                &run.text,
                run.size,
                run.value,
                [run.x as f32, run.baseline as f32],
                colors.paint(run.paint),
                colors.shade(run.shade),
            );
        }
        let mut bytes = Vec::with_capacity(pixels.len() * 4);
        for [r, g, b, a] in pixels {
            let byte = |v: f32| (v.clamp(0., 1.) * 255. + 0.5) as u8;
            bytes.extend([byte(b), byte(g), byte(r), byte(a)]);
        }
        Some(bytes)
    }
}

impl Fonts for Painter {
    fn width(&self, text: &str, size: i32, value: bool) -> i32 {
        if text.is_empty() {
            return 0;
        }
        let mut metrics = DWRITE_TEXT_METRICS::default();
        self.text(text, size, value)
            .and_then(|layout| unsafe { layout.GetMetrics(&mut metrics) }.ok())
            .map_or(0, |_| {
                metrics.widthIncludingTrailingWhitespace.ceil() as i32
            })
    }

    fn cap(&self, size: i32) -> f32 {
        self.cap * size as f32
    }
}

/// Paints `color` at `a` over a premultiplied pixel.
fn blend(pixel: &mut [f32; 4], color: Rgb, a: f32) {
    if a <= 0. {
        return;
    }
    for channel in 0..3 {
        pixel[channel] = color[channel] * a + pixel[channel] * (1. - a);
    }
    pixel[3] = a + pixel[3] * (1. - a);
}

// The interface's alpha correction (`gpui-pre-windows` `direct_write.rs`,
// after `alpha_correction.hlsl`): grey-scale coverage looks right only once
// corrected for the display's gamma and, for dark text, its contrast.

fn color_brightness([red, green, blue]: Rgb) -> f32 {
    red * 0.30 + green * 0.59 + blue * 0.11
}

fn light_on_dark_contrast(enhanced_contrast: f32, brightness: f32) -> f32 {
    enhanced_contrast * (4.0 * (0.75 - brightness)).clamp(0.0, 1.0)
}

fn enhance_contrast(alpha: f32, contrast: f32) -> f32 {
    alpha * (contrast + 1.0) / (alpha * contrast + 1.0)
}

fn apply_alpha_correction(alpha: f32, brightness: f32, gamma_ratios: [f32; 4]) -> f32 {
    let brightness_adjustment = gamma_ratios[0] * brightness + gamma_ratios[1];
    let correction =
        brightness_adjustment * alpha + (gamma_ratios[2] * brightness + gamma_ratios[3]);
    alpha + alpha * (1.0 - alpha) * correction
}

/// Where one run's glyphs go while DirectWrite hands them over.
struct Target<'a> {
    painter: &'a Painter,
    pixels: &'a mut [[f32; 4]],
    width: i32,
    height: i32,
    /// The row's baseline, pixels from the image's top.
    baseline: f32,
    color: Rgb,
    alpha: f32,
}

impl Target<'_> {
    /// Rasterizes a run of glyphs as the interface does: sampled three
    /// times per pixel across, averaged into one coverage, and corrected.
    fn glyphs(&mut self, run: &DWRITE_GLYPH_RUN, x: f32) -> windows::core::Result<()> {
        let Some(face) = run.fontFace.as_ref() else {
            return Ok(());
        };
        let face: IDWriteFontFace3 = face.cast()?;
        let (mut mode, mut grid) = (
            DWRITE_RENDERING_MODE1::default(),
            DWRITE_GRID_FIT_MODE::default(),
        );
        unsafe {
            face.GetRecommendedRenderingMode(
                run.fontEmSize,
                96.,
                96.,
                None,
                false,
                DWRITE_OUTLINE_THRESHOLD_ANTIALIASED,
                DWRITE_MEASURING_MODE_NATURAL,
                None,
                &mut mode,
                &mut grid,
            )?;
        }
        // Glyph run analysis does not take outlines, which large sizes get.
        if mode == DWRITE_RENDERING_MODE1_OUTLINE {
            mode = DWRITE_RENDERING_MODE1_NATURAL_SYMMETRIC;
        }
        let (antialias, texture, stride) = if mode == DWRITE_RENDERING_MODE1_ALIASED {
            (
                DWRITE_TEXT_ANTIALIAS_MODE_GRAYSCALE,
                DWRITE_TEXTURE_ALIASED_1x1,
                1,
            )
        } else {
            (
                DWRITE_TEXT_ANTIALIAS_MODE_CLEARTYPE,
                DWRITE_TEXTURE_CLEARTYPE_3x1,
                3,
            )
        };
        let analysis = unsafe {
            self.painter.factory.CreateGlyphRunAnalysis(
                run,
                None,
                mode,
                DWRITE_MEASURING_MODE_NATURAL,
                grid,
                antialias,
                x,
                self.baseline,
            )?
        };
        let bounds = unsafe { analysis.GetAlphaTextureBounds(texture)? };
        let (columns, rows) = (bounds.right - bounds.left, bounds.bottom - bounds.top);
        if columns <= 0 || rows <= 0 {
            return Ok(());
        }
        let mut samples = vec![0u8; (columns * rows) as usize * stride];
        unsafe { analysis.CreateAlphaTexture(texture, &bounds, &mut samples)? };
        let [wr, wg, wb] = self.painter.weights;
        let brightness = color_brightness(self.color);
        let contrast = light_on_dark_contrast(self.painter.contrast, brightness);
        for row in 0..rows {
            let y = bounds.top + row;
            if y < 0 || y >= self.height {
                continue;
            }
            for column in 0..columns {
                let x = bounds.left + column;
                if x < 0 || x >= self.width {
                    continue;
                }
                let at = (row * columns + column) as usize * stride;
                let coverage = if stride == 3 {
                    ((wr * u16::from(samples[at])
                        + wg * u16::from(samples[at + 1])
                        + wb * u16::from(samples[at + 2])
                        + 128)
                        >> 8) as f32
                        / 255.
                } else {
                    f32::from(samples[at]) / 255.
                };
                if coverage <= 0. {
                    continue;
                }
                let corrected = apply_alpha_correction(
                    enhance_contrast(coverage, contrast),
                    brightness,
                    self.painter.gamma,
                );
                let pixel = &mut self.pixels[(y * self.width + x) as usize];
                blend(pixel, self.color, corrected.clamp(0., 1.) * self.alpha);
            }
        }
        Ok(())
    }
}

/// Hands DirectWrite's glyph runs to the `Target` they are drawn for.
#[implement(IDWriteTextRenderer)]
struct Renderer;

#[allow(non_snake_case)]
impl IDWritePixelSnapping_Impl for Renderer_Impl {
    fn IsPixelSnappingDisabled(
        &self,
        _context: *const std::ffi::c_void,
    ) -> windows::core::Result<BOOL> {
        Ok(BOOL(0))
    }

    fn GetCurrentTransform(
        &self,
        _context: *const std::ffi::c_void,
        transform: *mut DWRITE_MATRIX,
    ) -> windows::core::Result<()> {
        unsafe {
            *transform = DWRITE_MATRIX {
                m11: 1.,
                m12: 0.,
                m21: 0.,
                m22: 1.,
                dx: 0.,
                dy: 0.,
            };
        }
        Ok(())
    }

    fn GetPixelsPerDip(&self, _context: *const std::ffi::c_void) -> windows::core::Result<f32> {
        Ok(1.)
    }
}

#[allow(non_snake_case)]
impl IDWriteTextRenderer_Impl for Renderer_Impl {
    fn DrawGlyphRun(
        &self,
        context: *const std::ffi::c_void,
        x: f32,
        _y: f32,
        _measuring: DWRITE_MEASURING_MODE,
        run: *const DWRITE_GLYPH_RUN,
        _description: *const DWRITE_GLYPH_RUN_DESCRIPTION,
        _effect: Ref<IUnknown>,
    ) -> windows::core::Result<()> {
        if context.is_null() || run.is_null() {
            return Ok(());
        }
        let target = unsafe { &mut *(context.cast::<Target>().cast_mut()) };
        target.glyphs(unsafe { &*run }, x)
    }

    fn DrawUnderline(
        &self,
        _context: *const std::ffi::c_void,
        _x: f32,
        _y: f32,
        _underline: *const DWRITE_UNDERLINE,
        _effect: Ref<IUnknown>,
    ) -> windows::core::Result<()> {
        Ok(())
    }

    fn DrawStrikethrough(
        &self,
        _context: *const std::ffi::c_void,
        _x: f32,
        _y: f32,
        _strikethrough: *const DWRITE_STRIKETHROUGH,
        _effect: Ref<IUnknown>,
    ) -> windows::core::Result<()> {
        Ok(())
    }

    fn DrawInlineObject(
        &self,
        _context: *const std::ffi::c_void,
        _x: f32,
        _y: f32,
        _object: Ref<IDWriteInlineObject>,
        _sideways: BOOL,
        _right_to_left: BOOL,
        _effect: Ref<IUnknown>,
    ) -> windows::core::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Cell, Fonts, Group, Paint, Painter, Run, Shade, Tone, columns, font_size, lay_out, split,
    };
    use crate::settings::TaskbarStyle as Style;

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
            high: false,
        }
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
            high: false,
        };
        Group {
            cells: vec![rate("↓", down), rate("↑", up)],
            tone: Tone::Network,
            history: vec![0.1, 0.3],
        }
    }

    fn names(columns: &[Vec<&Cell>]) -> Vec<Vec<String>> {
        columns
            .iter()
            .map(|column| column.iter().map(|cell| cell.label.clone()).collect())
            .collect()
    }

    fn values(runs: &[Run]) -> Vec<&Run> {
        runs.iter()
            .filter(|run| run.value && run.shade == Shade::Full)
            .collect()
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
        let layout = lay_out(&groups, 48, 1., Style::Labels, &Even, None);
        // Padding 8, name 30, gap 6, the widest value "100%" 40, padding 8.
        assert_eq!(layout.width, 8 + 30 + 6 + 40 + 8);
        assert_eq!(layout.font, 13);
        assert!(layout.tiles.is_empty());
        // Two rows of 17 px in the middle of 48, the capitals centred.
        let shown = values(&layout.runs);
        assert_eq!((shown[0].x, shown[0].baseline), (8 + 76 - 20, 7 + 13));
        assert_eq!((shown[1].x, shown[1].baseline), (8 + 76 - 30, 24 + 13));
        // The same readings with other values keep the size.
        let busier = vec![single("CPU", "100%"), single("MEM", "99%")];
        assert_eq!(
            lay_out(&busier, 48, 1., Style::Labels, &Even, None).width,
            layout.width
        );
        // A taskbar too low for two rows sets them side by side.
        let low = lay_out(&groups, 30, 1., Style::Labels, &Even, None);
        assert_eq!(low.width, 8 + 76 + 14 + 76 + 8);
        assert!(low.runs.iter().all(|run| run.baseline == 6 + 13));
        // At 150 % everything grows with the scale.
        assert_eq!(font_size(1.5), 20);
    }

    #[test]
    fn curves_set_each_reading_on_a_tile_over_its_curve() {
        let groups = vec![single("CPU", "1%"), rates("1 KB/s", "2 KB/s")];
        let layout = lay_out(&groups, 48, 1., Style::Curves, &Even, None);
        assert_eq!(layout.tiles.len(), 2);
        // Name 30 and the widest figure "100" with its unit "%", 40, inside
        // 8 px either side and 8 apart; tiles 38 px high in the middle.
        assert_eq!(layout.tiles[0].rect, [8, 5, 8 + 94, 43]);
        assert_eq!(layout.tiles[0].curve, [9, 23, 101, 42]);
        assert_eq!(layout.tiles[0].paint, Paint::Tone(Tone::Cpu));
        assert_eq!(layout.tiles[0].history, [0.2, 0.6, 0.4]);
        // A pair of rates: its marks, then the widest "999.9 MB/s".
        assert_eq!(layout.tiles[1].rect, [108, 5, 108 + 132, 43]);
        assert_eq!(layout.tiles[1].curve[1], 5 + 26);
        assert_eq!(layout.width, 240 + 8);
        // The unit is smaller and quieter than its figure.
        let unit = layout.runs.iter().find(|run| run.text == "%").unwrap();
        assert_eq!((unit.size, unit.shade), (11, Shade::Unit));
        // Other values keep the tiles where they are; a high one turns the
        // tile's curve amber.
        let mut busier = vec![single("CPU", "100%"), rates("999.9 MB/s", "0 KB/s")];
        busier[0].cells[0].high = true;
        let again = lay_out(&busier, 48, 1., Style::Curves, &Even, None);
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
        let layout = lay_out(&[single("CPU", "41%")], 48, 1., Style::Figures, &Even, None);
        let name = &layout.runs[0];
        assert_eq!((name.size, name.baseline), (10, 24 - 7));
        assert_eq!(name.paint, Paint::Tone(Tone::Cpu));
        let figure = values(&layout.runs)[0];
        assert_eq!(
            (figure.text.as_str(), figure.size, figure.baseline),
            ("41", 16, 24 + 13)
        );
        // The figure starts under its name.
        assert_eq!(figure.x, name.x);
        assert_eq!(layout.width, 8 + 40 + 8);
        // A taskbar too low for two rows shows labels instead.
        let low = lay_out(&[single("CPU", "41%")], 30, 1., Style::Figures, &Even, None);
        assert!(low.runs.iter().all(|run| run.size == 13));
    }

    #[test]
    fn capsules_line_up_in_one_row_and_keep_their_width() {
        let groups = vec![single("CPU", "1%"), rates("1 KB/s", "2 KB/s")];
        let layout = lay_out(&groups, 48, 1., Style::Capsules, &Even, None);
        assert_eq!(layout.pills.len(), 2);
        // 24 px high in the middle of 48. Inside 7, a dot of 6 and 4, the
        // name 30 and 4, the widest "100%" 40, inside 7: 98.
        assert_eq!(layout.pills[0].rect, [8, 12, 8 + 98, 36]);
        assert_eq!(layout.pills[0].dot, Some(([18., 24.], 3.)));
        assert_eq!(layout.pills[0].paint, Paint::Tone(Tone::Cpu));
        let (name, value) = (&layout.runs[0], values(&layout.runs)[0]);
        assert_eq!(value.x, name.x + 30 + 4);
        // Rates share one: each arrow 10 and 3 before the widest
        // "999.9 MB/s" 100, 7 between them.
        assert_eq!(
            layout.pills[1].rect,
            [110, 12, 110 + 7 + 113 + 7 + 113 + 7, 36]
        );
        assert!(layout.pills[1].dot.is_none());
        // Other values keep the capsules where they are.
        let busier = vec![single("CPU", "100%"), rates("999.9 MB/s", "0 KB/s")];
        assert_eq!(
            lay_out(&busier, 48, 1., Style::Capsules, &Even, None).pills,
            layout.pills
        );
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
                Style::Capsules,
                &Even,
                Some((&mut reserve, start + Duration::from_secs(seconds))),
            );
            layout.pills[0].rect[2] - layout.pills[0].rect[0]
        };
        // Only as wide as the value: inside 7, dot 6 and 4, name 30 and 4,
        // "1%" 20, inside 7.
        let narrow = width("1%", 0);
        assert_eq!(narrow, 78);
        // A wider value widens it at once…
        assert_eq!(width("100%", 5), narrow + 20);
        // …and its room holds for a minute after it.
        assert_eq!(width("1%", 30), narrow + 20);
        assert_eq!(width("1%", 64), narrow + 20);
        assert_eq!(width("1%", 66), narrow);
    }

    #[test]
    fn units_part_from_their_figures() {
        assert_eq!(split("12.4 MB/s"), ("12.4", " MB/s"));
        assert_eq!(split("40% TDP"), ("40", "% TDP"));
        assert_eq!(split("—"), ("—", ""));
        // An uptime stays whole.
        assert_eq!(split("5 d 14 h"), ("5 d 14 h", ""));
    }

    #[test]
    fn text_shows_in_its_runs_and_the_rest_keeps_a_trace_of_alpha() {
        let Some(painter) = Painter::new(16) else {
            return;
        };
        let groups = vec![Group {
            cells: vec![Cell {
                label: "CPU".into(),
                value: "88%".into(),
                widest: "100%".into(),
                high: true,
            }],
            ..Group::default()
        }];
        let layout = lay_out(&groups, 48, 1.25, Style::Labels, &painter, None);
        let pixels = painter.paint(&layout, false, false, 1.25).unwrap();
        assert_eq!(pixels.len(), (layout.width * layout.height * 4) as usize);
        let at = |x: i32, y: i32| {
            let index = ((y * layout.width + x) * 4) as usize;
            [
                pixels[index],
                pixels[index + 1],
                pixels[index + 2],
                pixels[index + 3],
            ]
        };
        // The corner holds no text, only the trace.
        assert_eq!(at(0, 0), [0, 0, 0, 1]);
        // The high value is amber on a dark taskbar (premultiplied, red the
        // strongest channel); the name is white.
        let strongest = |run: &Run| {
            let right = run.x + painter.width(&run.text, run.size, run.value);
            (run.baseline - run.size..run.baseline + 3)
                .flat_map(|y| (run.x..right).map(move |x| (x, y)))
                .map(|(x, y)| at(x, y))
                .max_by_key(|pixel| pixel[3])
                .unwrap()
        };
        let value = strongest(&layout.runs[1]);
        assert!(value[3] > 200 && value[2] > value[0], "{value:?}");
        let name = strongest(&layout.runs[0]);
        assert!(name[3] > 120 && name[0] == name[2], "{name:?}");
    }

    /// Readings as the taskbar shows them, for looking at the text and the
    /// styles (`ARGUS_TASKBAR_SAMPLES=<png path> cargo test -p argus --lib
    /// taskbar_samples -- --ignored`): each style on a dark and a light
    /// taskbar at 100 %, and beside it the same four times as large.
    #[test]
    #[ignore]
    fn taskbar_samples() {
        let Ok(path) = std::env::var("ARGUS_TASKBAR_SAMPLES") else {
            return;
        };
        let wave = |level: f32, swing: f32, seed: f32| {
            (0..40)
                .map(|i| {
                    let t = i as f32;
                    (level + swing * (0.6 * (t * 0.31 + seed).sin() + 0.3 * (t * 0.77).sin()))
                        .clamp(0.02, 0.98)
                })
                .collect::<Vec<_>>()
        };
        let reading = |label: &str, value: &str, widest: &str, high: bool, tone, history| Group {
            cells: vec![Cell {
                label: label.into(),
                value: value.into(),
                widest: widest.into(),
                high,
            }],
            tone,
            history,
        };
        let mut network = rates("12.4 MB/s", "860 KB/s");
        network.history = wave(0.35, 0.3, 4.4);
        let groups = vec![
            reading(
                "CPU",
                "41%",
                "100%",
                false,
                Tone::Cpu,
                wave(0.38, 0.22, 0.4),
            ),
            reading(
                "内存",
                "72%",
                "100%",
                false,
                Tone::Memory,
                wave(0.7, 0.04, 1.3),
            ),
            reading("GPU", "88%", "100%", true, Tone::Gpu, wave(0.74, 0.2, 2.2)),
            reading(
                "CPU",
                "63 °C",
                "100 °C",
                false,
                Tone::Cpu,
                wave(0.6, 0.08, 3.1),
            ),
            network,
        ];
        let painter = Painter::new(13).unwrap();
        let mut strips = Vec::new();
        for style in [
            Style::Labels,
            Style::Curves,
            Style::Capsules,
            Style::Figures,
        ] {
            let layout = lay_out(&groups, 48, 1., style, &painter, None);
            for (light, background) in [(false, [28u8, 28, 30]), (true, [238, 238, 240])] {
                let image = painter.paint(&layout, light, false, 1.).unwrap();
                strips.push((layout.width, layout.height, image, background));
            }
        }
        let width = strips.iter().map(|strip| strip.0).max().unwrap() as u32;
        let height = strips.iter().map(|strip| strip.1).sum::<i32>() as u32;
        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        for (strip_width, strip_height, image, background) in &strips {
            for y in 0..*strip_height {
                for x in 0..width as i32 {
                    let [b, g, r, a] = if x < *strip_width {
                        let at = ((y * strip_width + x) * 4) as usize;
                        [image[at], image[at + 1], image[at + 2], image[at + 3]]
                    } else {
                        [0, 0, 0, 0]
                    };
                    let over = |source: u8, under: u8| {
                        (u32::from(source) + u32::from(under) * (255 - u32::from(a)) / 255).min(255)
                            as u8
                    };
                    rgba.extend([
                        over(r, background[0]),
                        over(g, background[1]),
                        over(b, background[2]),
                        255,
                    ]);
                }
            }
        }
        let zoom = 4usize;
        let mut large = Vec::with_capacity(rgba.len() * zoom * zoom);
        for y in 0..(height as usize * zoom) {
            for x in 0..(width as usize * zoom) {
                let at = ((y / zoom) * width as usize + x / zoom) * 4;
                large.extend_from_slice(&rgba[at..at + 4]);
            }
        }
        let save = |pixels: Vec<u8>, width: u32, height: u32, path: &str| {
            tiny_skia::Pixmap::from_vec(
                pixels,
                tiny_skia::IntSize::from_wh(width, height).unwrap(),
            )
            .unwrap()
            .save_png(path)
            .unwrap();
        };
        save(rgba, width, height, &path);
        save(
            large,
            width * zoom as u32,
            height * zoom as u32,
            &path.replace(".png", "-4x.png"),
        );
    }
}
