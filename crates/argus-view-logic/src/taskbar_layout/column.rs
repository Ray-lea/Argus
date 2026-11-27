//! The taskbar monitor down a vertical taskbar: each reading a stack of
//! short lines across the taskbar's width, its name over its value, in each
//! of the four styles:
//!
//! - labels: the name in its device's colour over the value;
//! - figures: the same, the figure larger;
//! - curves: a tile per reading, its name and value over its recent curve;
//! - capsules: a capsule per reading, a dot in its device's colour before
//!   its name.
//!
//! A pair of rates stacks its two, each under its arrow. A value too wide
//! for the column with its unit beside its figure takes the unit up to its
//! name's line ("↓ MB/s" over "12.4"), or onto a line of its own under the
//! figure where the name leaves no room for it, rather than set the name
//! too small to read; a large figure is first set up to a fifth smaller to
//! keep its unit, and a figure wider than the column is set smaller. All of
//! it follows the widest value a reading may take, so that the column
//! keeps its height as the values change. A value keeps the room
//! it took lately (`Reserve`), its widest without one, and lies at the right
//! of that room, centred in the column: changing values seldom move it. A
//! taskbar narrower than 32 px (at 100 %) shows nothing.
use super::{
    Cell, Fonts, Group, Layout, Paint, Pill, Reserve, Run, Shade, Style, Tile, Tone, figure,
    figure_width, font_size, held, size_scale, split, value_paint,
};
use std::time::Instant;

/// Lays out `groups` in `style` down a vertical taskbar `width` pixels wide
/// at `scale`, its sizes following the width as a horizontal taskbar's
/// follow its height (`size_scale`); labels, capsules and figures keep
/// their recent room in `reserve` when given one, at the time given with
/// it, and the room of their widest values without. Only the readings that
/// fit in `room` pixels down the taskbar show, the first ones. Nothing to
/// show, or a taskbar too narrow, leaves the layout empty (no width).
pub fn lay_out_column(
    groups: &[Group],
    width: i32,
    scale: f32,
    style: Style,
    fonts: &impl Fonts,
    reserve: Option<(&mut Reserve, Instant)>,
    room: i32,
) -> Layout {
    let narrow = width < (32. * scale).round() as i32;
    let scale = size_scale(width, scale);
    let mut layout = Layout {
        width,
        font: font_size(scale),
        vertical: true,
        ..Layout::default()
    };
    if narrow || groups.iter().all(|group| group.cells.is_empty()) {
        layout.width = 0;
        return layout;
    }
    let (key, reserve) = match style {
        Style::Labels => ("column-labels", reserve),
        Style::Figures => ("column-figures", reserve),
        Style::Capsules => ("column-capsules", reserve),
        // A curve's tile keeps the room of its widest value, as on a
        // horizontal taskbar.
        Style::Curves => ("column-curves", None),
    };
    let mut column = Column {
        fonts,
        reserve,
        scale,
        key,
        left: 0,
        right: width,
        y: 0,
        runs: Vec::new(),
        ends: Vec::new(),
    };
    let pad = match style {
        Style::Labels => column.labels(groups),
        Style::Figures => column.figures(groups),
        Style::Curves => column.curves(groups, &mut layout.tiles),
        Style::Capsules => column.capsules(groups, &mut layout.pills),
    };
    // The readings that fit, each whole: a tile or a capsule each.
    let fit = column
        .ends
        .iter()
        .take_while(|(end, _)| end + pad <= room)
        .count();
    let Some(&(end, runs)) = fit.checked_sub(1).and_then(|last| column.ends.get(last)) else {
        layout.tiles.clear();
        layout.pills.clear();
        layout.width = 0;
        return layout;
    };
    column.runs.truncate(runs);
    layout.tiles.truncate(fit);
    layout.pills.truncate(fit);
    layout.height = end + pad;
    layout.runs = column.runs;
    layout
}

/// The sizes of a reading's text: its name, its figure and its unit, and
/// the least its figure is set to keep its unit beside it.
#[derive(Clone, Copy)]
struct Sizes {
    name: i32,
    value: i32,
    unit: i32,
    least: i32,
}

/// A reading's name line: its text, set in a value's weight (a rate's
/// arrow) or a name's, how it shows, and whether a dot in its device's
/// colour comes first (capsules).
struct Name<'t> {
    text: &'t str,
    value: bool,
    paint: Paint,
    shade: Shade,
    dot: Option<Paint>,
}

/// Where a value's unit goes.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Unit {
    /// After its figure (or no unit).
    Beside,
    /// After its name, on the line above.
    Up,
    /// On a line of its own under the figure.
    Below,
}

/// How a value is set: where its unit goes, and its figure's size.
struct Fit {
    unit: Unit,
    size: i32,
}

/// Lays lines down the column from the top, each centred between `left`
/// and `right`.
struct Column<'a, F: Fonts> {
    fonts: &'a F,
    reserve: Option<(&'a mut Reserve, Instant)>,
    scale: f32,
    /// The style, where a value keeps its room (`Cell::kept`).
    key: &'static str,
    left: i32,
    right: i32,
    /// Where the next line starts.
    y: i32,
    runs: Vec<Run>,
    /// Where each reading laid out ends, and the runs laid out by then.
    ends: Vec<(i32, usize)>,
}

/// A line's height for text `size` pixels high.
fn line(size: i32) -> i32 {
    (size as f32 * 1.2).round() as i32
}

/// A value's unit and its widest value's, without the space before them;
/// empty where they have none.
fn units(cell: &Cell) -> (&str, &str) {
    (
        split(&cell.value).1.trim_start(),
        split(&cell.widest).1.trim_start(),
    )
}

impl<F: Fonts> Column<'_, F> {
    fn px(&self, logical: f32) -> i32 {
        (logical * self.scale).round() as i32
    }

    /// A reading's sizes from their logical pixels.
    fn sizes(&self, name: f32, value: f32, unit: f32, least: f32) -> Sizes {
        Sizes {
            name: self.px(name),
            value: self.px(value),
            unit: self.px(unit),
            least: self.px(least),
        }
    }

    /// The baseline of text `size` high on a line `height` high from the
    /// current one's top: the capitals' middle on the line's middle.
    fn baseline(&self, height: i32, size: i32) -> i32 {
        self.y + ((height as f32 + self.fonts.cap(size)) / 2.).round() as i32
    }

    /// The largest size up to `size`, but at least 8 px at 100 %, that sets
    /// what `width` measures no wider than `room`.
    fn shrink(&self, size: i32, room: i32, width: impl Fn(i32) -> i32) -> i32 {
        let least = self.px(8.).min(size);
        let wide = width(size);
        if wide <= room {
            return size;
        }
        let mut size =
            ((size as f32 * room as f32 / wide.max(1) as f32).floor() as i32).clamp(least, size);
        while size > least && width(size) > room {
            size -= 1;
        }
        size
    }

    /// How `cell`'s value fits the column, as its widest and its present
    /// value both would: with its unit beside its figure, the figure set
    /// smaller down to `sizes.least`; else with its unit after its name,
    /// where the name's line has room for it, or on a line of its own; the
    /// figure as large as fits. A value without a unit is set as small as
    /// it needs.
    fn fit(&self, cell: &Cell, name: &Name, sizes: Sizes) -> Fit {
        let fonts = self.fonts;
        let room = self.right - self.left;
        let whole = |size: i32| {
            figure_width(fonts, &cell.widest, size, sizes.unit).max(figure_width(
                fonts,
                &cell.value,
                size,
                sizes.unit,
            ))
        };
        let mut size = sizes.value;
        while size > sizes.least && whole(size) > room {
            size -= 1;
        }
        let units = units(cell);
        if whole(size) <= room || (units.0.is_empty() && units.1.is_empty()) {
            return Fit {
                unit: Unit::Beside,
                size: self.shrink(size, room, whole),
            };
        }
        let size = self.shrink(sizes.value, room, |size| {
            fonts
                .width(split(&cell.widest).0, size, true)
                .max(fonts.width(split(&cell.value).0, size, true))
        });
        let up = self.name_widths(name, Some(units), sizes, sizes.name).0 <= room;
        Fit {
            unit: if up { Unit::Up } else { Unit::Below },
            size,
        }
    }

    /// One reading: its name's line (with its unit, when the value is too
    /// wide for it beside the figure and the name leaves room), then its
    /// value's, then its unit's when it has a line of its own. The dot's
    /// centre and radius, when its name has one.
    fn reading(&mut self, cell: &Cell, name: Name, sizes: Sizes) -> Option<([f32; 2], f32)> {
        let fit = self.fit(cell, &name, sizes);
        let unit = (fit.unit == Unit::Up).then(|| units(cell));
        let dot = self.name_line(&name, cell, unit, sizes);
        self.value_line(cell, &fit, sizes);
        if fit.unit == Unit::Below {
            self.unit_line(cell, sizes);
        }
        dot
    }

    /// A name's line `size` high, with `unit` after it: its width, the
    /// name's width and the unit's size (as much smaller as the name).
    fn name_widths(
        &self,
        name: &Name,
        unit: Option<(&str, &str)>,
        sizes: Sizes,
        size: i32,
    ) -> (i32, i32, i32) {
        let (dot, dot_gap, unit_gap) = self.name_gaps(name);
        let unit_size = (sizes.unit * size + sizes.name / 2) / sizes.name.max(1);
        let text = self.fonts.width(name.text, size, name.value);
        let unit = unit.map_or(0, |(now, widest)| {
            self.fonts
                .width(now, unit_size, true)
                .max(self.fonts.width(widest, unit_size, true))
        });
        // The dot, the name and the unit, each that shows a gap after the
        // one before.
        let mut total = dot;
        if text > 0 {
            total += if dot > 0 { dot_gap } else { 0 } + text;
        }
        if unit > 0 {
            total += match (dot > 0, text > 0) {
                (_, true) => unit_gap,
                (true, false) => dot_gap,
                (false, false) => 0,
            } + unit;
        }
        (total, text, unit_size)
    }

    /// A name's dot (none without one), the gap after it and the gap
    /// before a unit.
    fn name_gaps(&self, name: &Name) -> (i32, i32, i32) {
        (
            name.dot.map_or(0, |_| self.px(6.)),
            self.px(4.),
            self.px(3.),
        )
    }

    /// A name's line: a dot, the name, and the unit taken up from the
    /// value, centred together as the widest unit would be (so the name
    /// stays put as the unit changes); a name too wide alone is set
    /// smaller.
    fn name_line(
        &mut self,
        name: &Name,
        cell: &Cell,
        unit: Option<(&str, &str)>,
        sizes: Sizes,
    ) -> Option<([f32; 2], f32)> {
        let height = line(sizes.name);
        if name.text.is_empty() && name.dot.is_none() && unit.is_none() {
            return None;
        }
        let room = self.right - self.left;
        let (dot, dot_gap, unit_gap) = self.name_gaps(name);
        let size = self.shrink(sizes.name, room, |size| {
            self.name_widths(name, unit, sizes, size).0
        });
        let (total, text, unit_size) = self.name_widths(name, unit, sizes, size);
        let baseline = self.baseline(height, size);
        let mut x = (self.left + self.right - total) / 2;
        let mut centre = None;
        if dot > 0 {
            let radius = dot as f32 / 2.;
            centre = Some((
                [x as f32 + radius, self.y as f32 + height as f32 / 2.],
                radius,
            ));
            x += dot + dot_gap;
        }
        if text > 0 {
            self.runs.push(Run {
                text: name.text.to_owned(),
                x,
                baseline,
                size,
                value: name.value,
                paint: name.paint,
                shade: name.shade,
            });
            x += text + unit_gap;
        }
        if let Some((now, _)) = unit.filter(|(now, _)| !now.is_empty()) {
            self.runs.push(Run {
                text: now.to_owned(),
                x,
                baseline,
                size: unit_size,
                value: true,
                paint: value_paint(cell),
                shade: Shade::Unit,
            });
        }
        self.y += height;
        centre
    }

    /// A value's line: its figure, with its unit unless that has a line of
    /// its own, at the right of the room the value keeps, which is centred.
    fn value_line(&mut self, cell: &Cell, fit: &Fit, sizes: Sizes) {
        let height = line(sizes.value);
        let fonts = self.fonts;
        let alone = fit.unit != Unit::Beside;
        let shown = |value: &str| {
            if alone {
                fonts.width(split(value).0, fit.size, true)
            } else {
                figure_width(fonts, value, fit.size, sizes.unit)
            }
        };
        let (now, widest) = (shown(&cell.value), shown(&cell.widest));
        let room =
            held(&mut self.reserve, cell.kept(self.key), now, widest).min(self.right - self.left);
        let right = (self.left + self.right + room) / 2;
        let baseline = self.baseline(height, fit.size);
        if alone {
            self.runs.push(Run {
                text: split(&cell.value).0.to_owned(),
                x: right - now,
                baseline,
                size: fit.size,
                value: true,
                paint: value_paint(cell),
                shade: Shade::Full,
            });
        } else {
            figure(
                &mut self.runs,
                fonts,
                &cell.value,
                right,
                baseline,
                fit.size,
                sizes.unit,
                value_paint(cell),
            );
        }
        self.y += height;
    }

    /// A unit's line of its own, under its figure: centred, smaller and
    /// quieter, as beside it. An unknown value shows none, but keeps the
    /// line.
    fn unit_line(&mut self, cell: &Cell, sizes: Sizes) {
        let height = line(sizes.unit);
        let unit = units(cell).0;
        if !unit.is_empty() {
            let width = self.fonts.width(unit, sizes.unit, true);
            self.runs.push(Run {
                text: unit.to_owned(),
                x: (self.left + self.right - width) / 2,
                baseline: self.baseline(height, sizes.unit),
                size: sizes.unit,
                value: true,
                paint: value_paint(cell),
                shade: Shade::Unit,
            });
        }
        self.y += height;
    }

    /// A pair of rates, each under its arrow in its device's colour, a
    /// little apart.
    fn rates(&mut self, group: &Group, sizes: Sizes) {
        for (row, cell) in group.cells.iter().enumerate() {
            if row > 0 {
                self.y += self.px(3.);
            }
            let name = Name {
                text: &cell.label,
                value: true,
                paint: Paint::Tone(group.tone),
                shade: Shade::Full,
                dot: None,
            };
            self.reading(cell, name, sizes);
        }
    }

    /// A reading laid out: where it ends, for the column to end there.
    fn ended(&mut self) {
        self.ends.push((self.y, self.runs.len()));
    }

    /// Labels: a name in its device's colour over its value. The padding
    /// at the column's ends.
    fn labels(&mut self, groups: &[Group]) -> i32 {
        let sizes = self.sizes(12., 14., 11., 14.);
        let (side, pad, gap) = (self.px(4.), self.px(6.), self.px(7.));
        (self.left, self.right) = (side, self.right - side);
        self.y = pad;
        let mut before: Option<&Cell> = None;
        for group in groups.iter().filter(|group| !group.cells.is_empty()) {
            if before.is_some() {
                self.y += gap;
            }
            match group.cells.as_slice() {
                [cell] => {
                    // A reading of no device (an uptime) has no colour to
                    // take: its name stays a plain one's quieter ink.
                    let (paint, shade) = match group.tone {
                        Tone::Plain => (Paint::Ink, Shade::Name),
                        tone => (Paint::Tone(tone), Shade::Full),
                    };
                    let name = Name {
                        text: cell.tag_after(before),
                        value: false,
                        paint,
                        shade,
                        dot: None,
                    };
                    self.reading(cell, name, sizes);
                }
                _ => self.rates(group, sizes),
            }
            before = group.cells.last();
            self.ended();
        }
        pad
    }

    /// Figures: a small name in its device's colour over a large figure; a
    /// pair of rates as labels set them. The padding at the column's ends.
    fn figures(&mut self, groups: &[Group]) -> i32 {
        // A large figure is set up to a fifth smaller to keep its unit
        // beside it.
        let large = self.sizes(11., 20., 12., 16.);
        let body = self.sizes(12., 14., 11., 14.);
        let (side, pad, gap) = (self.px(4.), self.px(6.), self.px(8.));
        (self.left, self.right) = (side, self.right - side);
        self.y = pad;
        let mut any = false;
        for group in groups.iter().filter(|group| !group.cells.is_empty()) {
            if std::mem::replace(&mut any, true) {
                self.y += gap;
            }
            match group.cells.as_slice() {
                [cell] => {
                    // In its device's colour even when the value is amber.
                    let name = Name {
                        text: cell.tag(),
                        value: false,
                        paint: Paint::Tone(group.tone),
                        shade: Shade::Full,
                        dot: None,
                    };
                    self.reading(cell, name, large);
                }
                _ => self.rates(group, body),
            }
            self.ended();
        }
        pad
    }

    /// Curves: a tile per reading, its name and value over its recent
    /// curve; a pair of rates over theirs. The padding at the column's
    /// ends.
    fn curves(&mut self, groups: &[Group], tiles: &mut Vec<Tile>) -> i32 {
        let sizes = self.sizes(11., 13., 11., 13.);
        let (side, inside, pad, gap, curve) = (
            self.px(3.),
            self.px(3.),
            self.px(4.),
            self.px(4.),
            self.px(14.),
        );
        let (left, right) = (side, self.right - side);
        (self.left, self.right) = (left + inside, right - inside);
        self.y = pad;
        for group in groups.iter().filter(|group| !group.cells.is_empty()) {
            if !tiles.is_empty() {
                self.y += gap;
            }
            let top = self.y;
            self.y += self.px(4.);
            match group.cells.as_slice() {
                [cell] => {
                    let name = Name {
                        text: cell.tag(),
                        value: false,
                        paint: Paint::Ink,
                        shade: Shade::Name,
                        dot: None,
                    };
                    self.reading(cell, name, sizes);
                }
                _ => self.rates(group, sizes),
            }
            let from = self.y;
            self.y += curve;
            tiles.push(Tile {
                rect: [left, top, right, self.y],
                curve: [left + 1, from, right - 1, self.y - 1],
                history: group.history.clone(),
                paint: if group.cells.iter().any(|cell| cell.high) {
                    Paint::High
                } else {
                    Paint::Tone(group.tone)
                },
            });
            self.ended();
        }
        pad
    }

    /// Capsules: a capsule per reading, a dot in its device's colour
    /// before its name over its value; a pair of rates share one, each
    /// under its arrow. The padding at the column's ends.
    fn capsules(&mut self, groups: &[Group], pills: &mut Vec<Pill>) -> i32 {
        let sizes = self.sizes(11., 14., 11., 14.);
        let (side, inside, pad, gap, ends) = (
            self.px(3.),
            self.px(3.),
            self.px(4.),
            self.px(4.),
            self.px(7.),
        );
        let (left, right) = (side, self.right - side);
        (self.left, self.right) = (left + inside, right - inside);
        self.y = pad;
        for group in groups.iter().filter(|group| !group.cells.is_empty()) {
            if !pills.is_empty() {
                self.y += gap;
            }
            let top = self.y;
            self.y += ends;
            // The dot marks the device, as a name does: in its colour even
            // when the value is amber.
            let paint = Paint::Tone(group.tone);
            let dot = match group.cells.as_slice() {
                [cell] => {
                    let name = Name {
                        text: cell.tag(),
                        value: false,
                        paint: Paint::Ink,
                        shade: Shade::Name,
                        dot: Some(paint),
                    };
                    self.reading(cell, name, sizes)
                }
                _ => {
                    self.rates(group, sizes);
                    None
                }
            };
            self.y += ends;
            pills.push(Pill {
                rect: [left, top, right, self.y],
                dot,
                paint,
            });
            self.ended();
        }
        pad
    }
}

#[cfg(test)]
mod tests {
    use super::lay_out_column as lay_out_in;
    use crate::taskbar_layout::{Cell, Fonts, Group, Layout, Paint, Reserve, Run, Shade, Tone};
    use argus_app::settings::TaskbarStyle as Style;
    use std::time::{Duration, Instant};

    /// Laid out with all the room it takes.
    fn lay_out_column(
        groups: &[Group],
        width: i32,
        scale: f32,
        style: Style,
        fonts: &Scaled,
        reserve: Option<(&mut Reserve, Instant)>,
    ) -> Layout {
        lay_out_in(groups, width, scale, style, fonts, reserve, i32::MAX)
    }

    #[test]
    fn only_the_first_readings_that_fit_the_room_show() {
        let groups = readings("41%", "12.4 MB/s");
        for style in STYLES {
            let whole = lay_out_column(&groups, 48, 1., style, &Scaled, None);
            // Room for all but the last reading by a pixel: the rates go,
            // their tile or capsule with them.
            let short = lay_out_in(&groups, 48, 1., style, &Scaled, None, whole.height - 1);
            assert!(short.width > 0 && short.height < whole.height, "{style:?}");
            assert!(short.runs.iter().all(|run| run.text != "↓"), "{style:?}");
            assert_eq!(short.runs, whole.runs[..short.runs.len()], "{style:?}");
            assert_eq!(
                short.tiles.len() + short.pills.len(),
                whole.tiles.len().min(2) + whole.pills.len().min(2)
            );
            // Exactly its height holds it all; too little for one, none.
            assert_eq!(
                lay_out_in(&groups, 48, 1., style, &Scaled, None, whole.height),
                whole
            );
            assert_eq!(
                lay_out_in(&groups, 48, 1., style, &Scaled, None, 20).width,
                0
            );
        }
        // Labels: CPU ends at 6 + 31, and with its padding takes 43.
        let one = lay_out_in(&groups, 48, 1., Style::Labels, &Scaled, None, 43);
        assert_eq!((one.height, one.runs.len()), (43, 3));
    }

    /// Every character `size` × 0.6 wide; capitals 0.7 of the size high.
    struct Scaled;

    impl Fonts for Scaled {
        fn width(&self, text: &str, size: i32, _value: bool) -> i32 {
            text.chars().count() as i32 * size * 6 / 10
        }

        fn cap(&self, size: i32) -> f32 {
            size as f32 * 0.7
        }
    }

    fn single(label: &str, value: &str, widest: &str, tone: Tone) -> Group {
        Group {
            cells: vec![Cell {
                label: label.into(),
                key: label.into(),
                value: value.into(),
                widest: widest.into(),
                ..Cell::default()
            }],
            tone,
            history: vec![0.2, 0.6, 0.4],
        }
    }

    fn rates(down: &str, up: &str) -> Group {
        let rate = |label: &str, value: &str| Cell {
            label: label.into(),
            key: "network".into(),
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

    fn readings(cpu: &str, down: &str) -> Vec<Group> {
        vec![
            single("CPU", cpu, "100%", Tone::Cpu),
            single("RAM", "72%", "100%", Tone::Memory),
            rates(down, "860 KB/s"),
        ]
    }

    fn run<'a>(layout: &'a Layout, text: &str) -> &'a Run {
        layout
            .runs
            .iter()
            .find(|run| run.text == text)
            .unwrap_or_else(|| panic!("no run {text:?} in {:?}", layout.runs))
    }

    fn right(run: &Run) -> i32 {
        run.x + Scaled.width(&run.text, run.size, run.value)
    }

    const STYLES: [Style; 4] = [
        Style::Labels,
        Style::Figures,
        Style::Curves,
        Style::Capsules,
    ];

    #[test]
    fn readings_stack_a_name_over_its_value_across_the_width() {
        let layout = lay_out_column(
            &readings("41%", "12.4 MB/s"),
            48,
            1.,
            Style::Labels,
            &Scaled,
            None,
        );
        assert!(layout.vertical);
        assert_eq!(layout.width, 48);
        // The name centred: "CPU" 21 px at 12 px in 48.
        let name = run(&layout, "CPU");
        assert_eq!(
            (name.x, name.size, name.paint),
            (13, 12, Paint::Tone(Tone::Cpu))
        );
        // Its value under it, the widest "100%" (25 + 6) centred and the
        // value at its right.
        let figure = run(&layout, "41");
        assert!(figure.baseline > name.baseline);
        assert_eq!(right(run(&layout, "%")), (48 + 31) / 2);
        assert_eq!(figure.size, 14);
        // Lines of 14 and 17 px, 6 px from the top, 7 apart.
        assert_eq!(name.baseline, 6 + 11);
        assert_eq!(run(&layout, "RAM").baseline, 6 + 14 + 17 + 7 + 11);
        // Every run inside the column, 4 px from its edges.
        for run in &layout.runs {
            assert!(run.x >= 4 && right(run) <= 44, "{run:?}");
        }
        // Six lines and the pair's 3 px apart: two readings, two rates.
        assert_eq!(layout.height, 6 + 4 * (14 + 17) + 7 + 7 + 3 + 6);
    }

    #[test]
    fn a_rate_too_wide_for_its_unit_takes_it_up_beside_its_arrow() {
        let layout = lay_out_column(
            &readings("41%", "12.4 MB/s"),
            48,
            1.,
            Style::Labels,
            &Scaled,
            None,
        );
        let (arrow, unit) = (run(&layout, "↓"), run(&layout, "MB/s"));
        assert_eq!(arrow.baseline, unit.baseline);
        assert_eq!(
            (arrow.paint, unit.shade),
            (Paint::Tone(Tone::Network), Shade::Unit)
        );
        // The figure alone below, set smaller: the widest "999.9" is 42 px
        // at 14 px, 39 at 13.
        let figure = run(&layout, "12.4");
        assert!(figure.baseline > arrow.baseline);
        assert_eq!(figure.size, 13);
        assert_eq!(right(figure), (48 + 39) / 2);
        // A wider taskbar has room for the unit beside it, everything set
        // a quarter larger.
        let wide = lay_out_column(
            &readings("41%", "12.4 MB/s"),
            120,
            1.,
            Style::Labels,
            &Scaled,
            None,
        );
        assert_eq!(run(&wide, "12.4").size, 18);
        assert!(run(&wide, " MB/s").baseline > run(&wide, "↓").baseline);
        assert_eq!(run(&wide, " MB/s").baseline, run(&wide, "12.4").baseline);
    }

    #[test]
    fn changing_values_keep_the_column_and_its_names_where_they_are() {
        for style in STYLES {
            let calm = lay_out_column(&readings("1%", "0 KB/s"), 48, 1., style, &Scaled, None);
            let busy = lay_out_column(
                &readings("100%", "999.9 MB/s"),
                48,
                1.,
                style,
                &Scaled,
                None,
            );
            assert_eq!(calm.height, busy.height, "{style:?}");
            assert_eq!(
                calm.tiles.iter().map(|tile| tile.rect).collect::<Vec<_>>(),
                busy.tiles.iter().map(|tile| tile.rect).collect::<Vec<_>>()
            );
            assert_eq!(calm.pills, busy.pills, "{style:?}");
            for text in ["↓", "↑"] {
                assert_eq!(run(&calm, text).x, run(&busy, text).x, "{style:?} {text}");
            }
        }
    }

    #[test]
    fn a_value_keeps_the_room_it_took_lately() {
        let mut reserve = Reserve::default();
        let start = Instant::now();
        let mut lay = |cpu: &str, seconds: u64| {
            lay_out_column(
                &[single("CPU", cpu, "100%", Tone::Cpu)],
                48,
                1.,
                Style::Labels,
                &Scaled,
                Some((&mut reserve, start + Duration::from_secs(seconds))),
            )
        };
        // "7%" (8 + 6) centred alone, then "100%" (25 + 6) widens its room
        // at once, which holds a minute after it.
        assert_eq!(right(run(&lay("7%", 0), "%")), (48 + 14) / 2);
        assert_eq!(right(run(&lay("100%", 5), "%")), (48 + 31) / 2);
        assert_eq!(right(run(&lay("7%", 30), "%")), (48 + 31) / 2);
        assert_eq!(right(run(&lay("7%", 66), "%")), (48 + 14) / 2);
    }

    #[test]
    fn tiles_and_capsules_fill_the_width_one_under_another() {
        let groups = readings("41%", "12.4 MB/s");
        let curves = lay_out_column(&groups, 48, 1., Style::Curves, &Scaled, None);
        assert_eq!(curves.tiles.len(), 3);
        for pair in curves.tiles.windows(2) {
            assert_eq!(pair[1].rect[1] - pair[0].rect[3], 4);
        }
        let tile = &curves.tiles[0];
        assert_eq!((tile.rect[0], tile.rect[2]), (3, 45));
        // The curve along the tile's bottom, under the text.
        assert_eq!(tile.curve[3], tile.rect[3] - 1);
        assert!(
            curves
                .runs
                .iter()
                .filter(|run| run.baseline < tile.rect[3])
                .all(|run| run.baseline <= tile.curve[1])
        );
        assert_eq!(curves.height, curves.tiles[2].rect[3] + 4);
        let capsules = lay_out_column(&groups, 48, 1., Style::Capsules, &Scaled, None);
        assert_eq!(capsules.pills.len(), 3);
        // A dot before a reading's name, none for rates.
        let (dot, radius) = capsules.pills[0].dot.unwrap();
        assert_eq!(radius, 3.);
        assert!(dot[0] + radius < run(&capsules, "CPU").x as f32);
        assert!(capsules.pills[2].dot.is_none());
    }

    #[test]
    fn figures_set_the_figure_as_large_as_fits() {
        let layout = lay_out_column(
            &readings("41%", "1 KB/s"),
            48,
            1.,
            Style::Figures,
            &Scaled,
            None,
        );
        // "100" at 20 px (36) and "%" at 12 (7) would take 43 of 40: the
        // figure is set at 18 px (32), its unit kept beside it.
        assert_eq!(run(&layout, "41").size, 18);
        assert_eq!(run(&layout, "%").baseline, run(&layout, "41").baseline);
        // On a wider taskbar it keeps its full size, a quarter larger.
        let wide = lay_out_column(
            &readings("41%", "1 KB/s"),
            72,
            1.,
            Style::Figures,
            &Scaled,
            None,
        );
        assert_eq!(run(&wide, "41").size, 25);
        assert_eq!(run(&wide, "%").baseline, run(&wide, "41").baseline);
    }

    #[test]
    fn a_unit_goes_up_beside_its_name_or_onto_a_line_under_its_figure() {
        // "RAM" (21) and "GB" (13) fit 40 with 3 between: the unit goes up,
        // the widest figure "999.9" set at 13 px to fit.
        let used = single("RAM", "23.3 GB", "999.9 GB", Tone::Memory);
        let layout = lay_out_column(&[used], 48, 1., Style::Labels, &Scaled, None);
        assert_eq!(run(&layout, "GB").baseline, run(&layout, "RAM").baseline);
        assert_eq!(run(&layout, "23.3").size, 13);
        // "Clock" (36) leaves no room for "GHz" (19): the unit takes a line
        // of its own under the figure, centred, and the name keeps 12 px.
        let clock = single("Clock", "4.52 GHz", "8.88 GHz", Tone::Cpu);
        let layout = lay_out_column(&[clock], 48, 1., Style::Labels, &Scaled, None);
        let (name, figure, unit) = (
            run(&layout, "Clock"),
            run(&layout, "4.52"),
            run(&layout, "GHz"),
        );
        assert_eq!((name.size, figure.size), (12, 14));
        assert!(unit.baseline > figure.baseline && figure.baseline > name.baseline);
        assert_eq!((unit.x, unit.shade), ((48 - 19) / 2, Shade::Unit));
        // Lines of 14, 17 and 13 px.
        assert_eq!(layout.height, 6 + 14 + 17 + 13 + 6);
    }

    #[test]
    fn a_value_without_a_unit_is_set_smaller_and_a_narrow_taskbar_shows_nothing() {
        let mut uptime = single("Up", "5 d 14 h", "99 d 23 h", Tone::Plain);
        uptime.cells[0].key = "uptime".into();
        let layout = lay_out_column(&[uptime.clone()], 48, 1., Style::Labels, &Scaled, None);
        let value = run(&layout, "5 d 14 h");
        // "99 d 23 h" is 9 characters: 7 px fit 37 of 40, kept at 8.
        assert_eq!(value.size, 8);
        assert_eq!(run(&layout, "Up").paint, Paint::Ink);
        assert_eq!(
            lay_out_column(&[uptime], 30, 1., Style::Labels, &Scaled, None).width,
            0
        );
        assert_eq!(
            lay_out_column(&[], 48, 1., Style::Labels, &Scaled, None).width,
            0
        );
    }
}
