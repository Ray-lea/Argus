//! The live tray icon, drawn at the size the shell asks for: the app's window
//! mark with its columns lit by the recent CPU loads, a hundred dots lit by
//! the current load, the load as a figure, or RunCat365's cat running at a
//! pace the load sets.
//!
//! The marks are laid out on whole pixels for 16, 20, 24 and 32 px (the
//! taskbar at 100–200 % scaling); other sizes take the layout that fills them
//! best, scaled by a whole factor and centred, so every dot stays sharp. A
//! figure comes from the system's text renderer (the `text` callback), which
//! returns its coverage. The cat is an image: its 32 px silhouettes are
//! averaged over each target pixel, and its colour says how much memory is
//! in use.

use std::sync::OnceLock;

/// How many recent loads a live icon keeps: the widest layout's columns.
pub(crate) const RECENT: usize = 6;

/// How many frames RunCat365's cat has.
pub(crate) const FRAMES: usize = 5;

/// What the icon shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Look {
    /// The window mark: a column of dots per recent sample, lit up to its
    /// load, the newest on the right.
    Load,
    /// A hundred dots (twenty-five below 32 px) lit as far as the load.
    Share,
    /// The load as a figure over a thin bar.
    Number,
    /// RunCat365's cat at frame `frame`, coloured by memory use.
    Runner,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Spec {
    pub look: Look,
    /// CPU load in percent; none before the first sample.
    pub load: Option<u8>,
    /// The recent loads, oldest first, ending with `load`.
    pub recent: [Option<u8>; RECENT],
    /// Memory in use in percent; the runner's colour.
    pub memory: Option<u8>,
    /// The runner's frame, wrapped into `FRAMES`.
    pub frame: u8,
    /// The taskbar is light: dark glyphs on it.
    pub light: bool,
    /// A new version is ready.
    pub badge: bool,
}

/// The load from which the data and the figure turn amber.
pub(crate) const HIGH: u8 = 80;

/// Where a figure goes, in pixels: left, top, right, bottom.
pub(crate) type Rect = [f32; 4];

type Rgb = [f32; 3];

fn hex(value: u32) -> Rgb {
    [
        ((value >> 16) & 255) as f32 / 255.,
        ((value >> 8) & 255) as f32 / 255.,
        (value & 255) as f32 / 255.,
    ]
}

struct Colors {
    ink: Rgb,
    load: Rgb,
    high: Rgb,
    accent: Rgb,
    critical: Rgb,
}

/// The interface's colours for each taskbar: the glyph matches the system's
/// own icons, the figure's bar the CPU series, the marks' data and the badge
/// the accent, the runner's colour the memory in use.
fn colors(light: bool) -> Colors {
    if light {
        Colors {
            ink: hex(0x1a1a1a),
            load: hex(0x0a8bbd),
            high: hex(0xc27a00),
            accent: hex(0x0067c0),
            critical: hex(0xc42b1c),
        }
    } else {
        Colors {
            ink: hex(0xffffff),
            load: hex(0x46cff4),
            high: hex(0xf4b446),
            accent: hex(0x4cc2ff),
            critical: hex(0xff6b6b),
        }
    }
}

/// The data colour for a load: the accent, amber from `HIGH`.
fn tint(load: u8, colors: &Colors) -> Rgb {
    if load >= HIGH {
        colors.high
    } else {
        colors.accent
    }
}

/// Which colour band `memory` falls into: 0 below 70 %, 1 from 70 %, 2 from 90 %.
pub(crate) fn memory_tier(memory: Option<u8>) -> u8 {
    match memory.unwrap_or(0) {
        90.. => 2,
        70.. => 1,
        _ => 0,
    }
}

/// The runner's colour for a memory reading: the taskbar's ink below 70 %,
/// amber from 70 %, red from 90 %.
fn memory_tint(memory: Option<u8>, colors: &Colors) -> Rgb {
    match memory_tier(memory) {
        2 => colors.critical,
        1 => colors.high,
        _ => colors.ink,
    }
}

/// Premultiplied RGBA, one pixel per entry.
struct Canvas {
    size: u32,
    pixels: Vec<[f32; 4]>,
}

impl Canvas {
    fn new(size: u32) -> Self {
        Self {
            size,
            pixels: vec![[0.; 4]; (size * size) as usize],
        }
    }

    /// Paints `color` at `alpha` over the canvas, weighted by `coverage` at
    /// each pixel's centre (and its index, for coverage given per pixel).
    fn fill(&mut self, color: Rgb, alpha: f32, coverage: impl Fn(f32, f32, usize) -> f32) {
        for (index, pixel) in self.pixels.iter_mut().enumerate() {
            let x = (index as u32 % self.size) as f32 + 0.5;
            let y = (index as u32 / self.size) as f32 + 0.5;
            let a = alpha * coverage(x, y, index).clamp(0., 1.);
            if a <= 0. {
                continue;
            }
            for channel in 0..3 {
                pixel[channel] = color[channel] * a + pixel[channel] * (1. - a);
            }
            pixel[3] = a + pixel[3] * (1. - a);
        }
    }

    /// Clears what `coverage` covers, as far as it covers it.
    fn erase(&mut self, coverage: impl Fn(f32, f32) -> f32) {
        for (index, pixel) in self.pixels.iter_mut().enumerate() {
            let x = (index as u32 % self.size) as f32 + 0.5;
            let y = (index as u32 / self.size) as f32 + 0.5;
            let keep = 1. - coverage(x, y).clamp(0., 1.);
            for channel in pixel.iter_mut() {
                *channel *= keep;
            }
        }
    }

    /// Straight RGBA bytes, as icons store them.
    fn rgba(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.pixels.len() * 4);
        for pixel in &self.pixels {
            let a = pixel[3];
            for channel in &pixel[..3] {
                let straight = if a > 0. { channel / a } else { 0. };
                bytes.push((straight.clamp(0., 1.) * 255.).round() as u8);
            }
            bytes.push((a.clamp(0., 1.) * 255.).round() as u8);
        }
        bytes
    }
}

/// How much of the pixel at (x, y) a disc covers.
fn disc(x: f32, y: f32, cx: f32, cy: f32, r: f32) -> f32 {
    (r + 0.5 - ((x - cx).powi(2) + (y - cy).powi(2)).sqrt()).clamp(0., 1.)
}

/// How much of the pixel at (x, y) a rounded rectangle covers.
fn rounded(x: f32, y: f32, rect: Rect, radius: f32) -> f32 {
    let [left, top, right, bottom] = rect;
    let (hw, hh) = ((right - left) / 2., (bottom - top) / 2.);
    let radius = radius.min(hw).min(hh).max(0.);
    let qx = (x - (left + hw)).abs() - (hw - radius);
    let qy = (y - (top + hh)).abs() - (hh - radius);
    let outside = (qx.max(0.).powi(2) + qy.max(0.).powi(2)).sqrt();
    let distance = outside + qx.max(qy).min(0.) - radius;
    (0.5 - distance).clamp(0., 1.)
}

/// A layout drawn for `size` pixels, placed on a larger icon: scaled by a
/// whole factor and centred on whole pixels.
#[derive(Clone, Copy)]
struct Place {
    scale: f32,
    offset: f32,
}

impl Place {
    /// Of `sizes` (the layouts' own), the one that fills `size` best, and how.
    fn fit(sizes: &[u32], size: u32) -> (usize, Place) {
        // Below the smallest layout (the shell never asks), that one, unscaled.
        let mut best = (0, u32::MAX, sizes[0], 1);
        for (index, &own) in sizes.iter().enumerate() {
            if own > size {
                continue;
            }
            let factor = size / own;
            let left = size - own * factor;
            // The least room left over; of equals, the layout made bigger.
            if (left, u32::MAX - own) < (best.1, u32::MAX - best.2) {
                best = (index, left, own, factor);
            }
        }
        let (index, left, _, factor) = best;
        let left = if left == u32::MAX { 0 } else { left };
        (
            index,
            Place {
                scale: factor as f32,
                offset: (left / 2) as f32,
            },
        )
    }

    fn at(self, value: f32) -> f32 {
        self.offset + value * self.scale
    }

    fn rect(self, left: f32, top: f32, side_x: f32, side_y: f32) -> Rect {
        [
            self.at(left),
            self.at(top),
            self.at(left + side_x),
            self.at(top + side_y),
        ]
    }
}

/// The window mark at one size, in whole pixels.
struct Window {
    size: u32,
    /// The tile: left, top, side, corner radius.
    tile: [f32; 4],
    dot: f32,
    pitch: f32,
    columns: usize,
    rows: usize,
    /// The first dot's top left corner.
    origin: (f32, f32),
    /// The window controls: side, top, first left edge, step.
    controls: [f32; 4],
    /// The update badge in the title area: centre and radius.
    badge: [f32; 3],
}

const WINDOWS: [Window; 4] = [
    Window {
        size: 16,
        tile: [1., 1., 14., 3.5],
        dot: 2.,
        pitch: 3.,
        columns: 4,
        rows: 3,
        origin: (3., 6.),
        controls: [1., 3., 3., 2.],
        badge: [12., 3., 2.5],
    },
    Window {
        size: 20,
        tile: [1., 1., 18., 4.5],
        dot: 2.,
        pitch: 3.,
        columns: 5,
        rows: 3,
        origin: (3., 9.),
        controls: [2., 4., 3., 3.],
        badge: [15., 5., 3.],
    },
    Window {
        size: 24,
        tile: [2., 2., 20., 5.5],
        dot: 2.,
        pitch: 3.,
        columns: 5,
        rows: 4,
        origin: (5., 8.),
        controls: [2., 4., 5., 3.],
        badge: [18., 6., 3.5],
    },
    Window {
        size: 32,
        tile: [2., 2., 28., 7.],
        dot: 2.,
        pitch: 4.,
        columns: 6,
        rows: 5,
        origin: (5., 9.),
        controls: [2., 5., 5., 3.],
        badge: [24., 8., 4.5],
    },
];

/// How many of `rows` dots a load lights: at least one once sampled.
fn lit(load: u8, rows: usize) -> usize {
    ((f32::from(load.min(100)) / 100. * rows as f32).round() as usize).clamp(1, rows)
}

/// The window mark as a glyph in the taskbar's ink: lit dots and window
/// controls cut out of the tile, the newest column filled with the data
/// colour.
fn window(canvas: &mut Canvas, spec: &Spec, colors: &Colors) {
    let sizes = WINDOWS.map(|w| w.size);
    let (index, place) = Place::fit(&sizes, canvas.size);
    let w = &WINDOWS[index];
    let [left, top, side, radius] = w.tile;
    let tile = place.rect(left, top, side, side);
    canvas.fill(colors.ink, 1., |x, y, _| {
        rounded(x, y, tile, radius * place.scale)
    });
    let corner = w.dot * place.scale / 4.;
    let dot = |column: usize, row: usize| {
        place.rect(
            w.origin.0 + column as f32 * w.pitch,
            w.origin.1 + row as f32 * w.pitch,
            w.dot,
            w.dot,
        )
    };
    let shown = &spec.recent[RECENT - w.columns..];
    let mut newest = Vec::new();
    for (column, load) in shown.iter().enumerate() {
        let Some(load) = *load else {
            continue;
        };
        for row in w.rows - lit(load, w.rows)..w.rows {
            let rect = dot(column, row);
            canvas.erase(|x, y| rounded(x, y, rect, corner));
            if column == w.columns - 1 {
                newest.push(rect);
            }
        }
    }
    let [control, control_top, control_left, step] = w.controls;
    for n in 0..3 {
        let rect = place.rect(
            control_left + n as f32 * step,
            control_top,
            control,
            control,
        );
        canvas.erase(|x, y| rounded(x, y, rect, control * place.scale / 4.));
    }
    if let Some(load) = spec.recent[RECENT - 1] {
        let color = tint(load, colors);
        for rect in newest {
            canvas.fill(color, 1., |x, y, _| rounded(x, y, rect, corner));
        }
    }
    if spec.badge {
        let [cx, cy, r] = w.badge;
        let (cx, cy, r) = (place.at(cx), place.at(cy), r * place.scale);
        canvas.erase(|x, y| disc(x, y, cx, cy, r + 1.5));
        canvas.fill(colors.accent, 1., |x, y, _| disc(x, y, cx, cy, r));
    }
}

/// The hundred dots at one size: a side of five (each dot 4 %) up to 24 px,
/// the full ten at 32.
struct Hundred {
    size: u32,
    side: usize,
    dot: f32,
    pitch: f32,
    origin: f32,
}

const HUNDREDS: [Hundred; 4] = [
    Hundred {
        size: 16,
        side: 5,
        dot: 2.,
        pitch: 3.,
        origin: 1.,
    },
    Hundred {
        size: 20,
        side: 5,
        dot: 2.,
        pitch: 4.,
        origin: 1.,
    },
    Hundred {
        size: 24,
        side: 5,
        dot: 2.,
        pitch: 5.,
        origin: 1.,
    },
    Hundred {
        size: 32,
        side: 10,
        dot: 2.,
        pitch: 3.,
        origin: 1.,
    },
];

/// Whether the dot in `row`, `column` belongs to the A the dots draw.
fn in_a(side: usize, column: usize, row: usize) -> bool {
    if side == 5 {
        return match row {
            0 => column == 2,
            1 => column == 1 || column == 3,
            2 => (1..=3).contains(&column),
            _ => column == 0 || column == 4,
        };
    }
    // The 10 × 10 lattice: two legs leaning in, a pair at the apex, a bar.
    let (x, y) = (column as f32, row as f32);
    let centre = 4.5 - 0.5 * y;
    let leg = |at: f32| (1. - (at - centre).abs() * 1.15).clamp(0., 1.);
    (row == 0 && (column == 4 || column == 5))
        || (row == 6 && (2..=7).contains(&column))
        || leg(x).max(leg(9. - x)) > 0.5
}

/// The hundred dots: lit from the bottom row up, left to right, as far as
/// the load; the rest in the taskbar's ink, the A stronger.
fn hundred(canvas: &mut Canvas, spec: &Spec, colors: &Colors) {
    let sizes = HUNDREDS.map(|h| h.size);
    let (index, place) = Place::fit(&sizes, canvas.size);
    let h = &HUNDREDS[index];
    let total = h.side * h.side;
    let lit = spec
        .load
        .map(|load| (f32::from(load.min(100)) / 100. * total as f32).round() as usize)
        .unwrap_or(0);
    let color = spec.load.map(|load| tint(load, colors));
    let corner = h.dot * place.scale / 4.;
    for row in 0..h.side {
        for column in 0..h.side {
            // The badge takes the top right dot's place.
            if spec.badge && row == 0 && column == h.side - 1 {
                continue;
            }
            let rect = place.rect(
                h.origin + column as f32 * h.pitch,
                h.origin + row as f32 * h.pitch,
                h.dot,
                h.dot,
            );
            let rank = (h.side - 1 - row) * h.side + column;
            let (paint, alpha) = match color {
                Some(color) if rank < lit => (color, 1.),
                _ if in_a(h.side, column, row) => (colors.ink, 0.42),
                _ => (colors.ink, 0.2),
            };
            canvas.fill(paint, alpha, |x, y, _| rounded(x, y, rect, corner));
        }
    }
    if spec.badge {
        let centre = h.origin + (h.side - 1) as f32 * h.pitch + h.dot / 2.;
        let (cx, cy) = (place.at(centre), place.at(h.origin + h.dot / 2.));
        let r = (h.dot * 0.9 + 0.6) * place.scale;
        canvas.fill(colors.accent, 1., |x, y, _| disc(x, y, cx, cy, r));
    }
}

/// The bar's place: whole pixels along the bottom, one pixel from the edges.
fn bar_rect(size: u32, thick: f32) -> Rect {
    let scale = size as f32 / 16.;
    let side = (1.5 * scale).floor().max(1.);
    let height = (thick * scale).round().max(2.);
    let bottom = size as f32 - (0.75 * scale).round().max(1.);
    [side, bottom - height, size as f32 - side, bottom]
}

fn bar(canvas: &mut Canvas, rect: Rect, load: Option<u8>, colors: &Colors) {
    let radius = (rect[3] - rect[1]) / 2.;
    canvas.fill(colors.ink, 0.22, |x, y, _| rounded(x, y, rect, radius));
    let Some(load) = load.filter(|load| *load > 0) else {
        return;
    };
    let tint = if load >= HIGH {
        colors.high
    } else {
        colors.load
    };
    let end = rect[0] + (rect[2] - rect[0]) * f32::from(load.min(100)) / 100.;
    canvas.fill(tint, 1., |x, y, _| {
        rounded(x, y, [rect[0], rect[1], end, rect[3]], radius)
    });
}

/// The runner's frames: RunCat365's five 32 px silhouettes, decoded once and
/// keeping only their coverage (the icon colours them).
struct Runner {
    coverage: [[u8; RUNNER_SIDE * RUNNER_SIDE]; FRAMES],
}

const RUNNER_SIDE: usize = 32;

fn frames() -> Option<&'static Runner> {
    static RUNNER: OnceLock<Option<Runner>> = OnceLock::new();
    RUNNER
        .get_or_init(|| {
            const SOURCES: [&[u8]; FRAMES] = [
                include_bytes!("../../../resources/tray/runner/cat_0.png"),
                include_bytes!("../../../resources/tray/runner/cat_1.png"),
                include_bytes!("../../../resources/tray/runner/cat_2.png"),
                include_bytes!("../../../resources/tray/runner/cat_3.png"),
                include_bytes!("../../../resources/tray/runner/cat_4.png"),
            ];
            let mut coverage = [[0u8; RUNNER_SIDE * RUNNER_SIDE]; FRAMES];
            for (cells, source) in coverage.iter_mut().zip(SOURCES) {
                let image = image::load_from_memory_with_format(source, image::ImageFormat::Png)
                    .ok()?
                    .to_rgba8();
                if (image.width() as usize, image.height() as usize) != (RUNNER_SIDE, RUNNER_SIDE) {
                    return None;
                }
                for (cell, pixel) in cells.iter_mut().zip(image.pixels()) {
                    *cell = pixel.0[3];
                }
            }
            Some(Runner { coverage })
        })
        .as_ref()
}

/// How much of the pixel at (x, y) the frame covers: the source cells under
/// it, weighted by their overlap with the pixel.
fn runner_coverage(frame: &[u8], size: u32, x: f32, y: f32) -> f32 {
    let side = RUNNER_SIDE as f32;
    let unit = side / size as f32;
    let left = ((x - 0.5) * unit).clamp(0., side);
    let top = ((y - 0.5) * unit).clamp(0., side);
    let right = (((x - 0.5) * unit) + unit).clamp(0., side);
    let bottom = (((y - 0.5) * unit) + unit).clamp(0., side);
    let (mut sum, mut area) = (0., 0.);
    for row in top.floor() as usize..bottom.ceil() as usize {
        let vertical = (bottom.min(row as f32 + 1.) - top.max(row as f32)).max(0.);
        for column in left.floor() as usize..right.ceil() as usize {
            let horizontal = (right.min(column as f32 + 1.) - left.max(column as f32)).max(0.);
            let weight = horizontal * vertical;
            sum += f32::from(frame[row * RUNNER_SIDE + column]) / 255. * weight;
            area += weight;
        }
    }
    if area > 0. { sum / area } else { 0. }
}

/// The runner: the cat in the memory colour, at frame `spec.frame`; the
/// badge sits where the figure's does.
fn runner(canvas: &mut Canvas, spec: &Spec, colors: &Colors) {
    let Some(runner) = frames() else {
        // Without the frames the mark stands in, as it does without text.
        return window(canvas, spec, colors);
    };
    let frame = &runner.coverage[usize::from(spec.frame) % FRAMES];
    let size = canvas.size;
    canvas.fill(memory_tint(spec.memory, colors), 1., |x, y, _| {
        runner_coverage(frame, size, x, y)
    });
    if spec.badge {
        let scale = size as f32 / 16.;
        let (cx, cy) = (13.2 * scale, 3. * scale);
        canvas.erase(|x, y| disc(x, y, cx, cy, 3.7 * scale));
        canvas.fill(colors.accent, 1., |x, y, _| disc(x, y, cx, cy, 2.5 * scale));
    }
}

/// Sets a figure in `font` pixels inside a rectangle and returns its
/// coverage, one byte per pixel of the icon, or none when it cannot.
pub(crate) type Text<'a> = dyn Fn(&str, Rect, f32) -> Option<Vec<u8>> + 'a;

/// The icon's straight RGBA pixels, `size` square.
pub(crate) fn render(spec: Spec, size: u32, text: &Text) -> Vec<u8> {
    let scale = size as f32 / 16.;
    let colors = colors(spec.light);
    let mut canvas = Canvas::new(size);
    match (spec.look, spec.load) {
        (Look::Share, _) => hundred(&mut canvas, &spec, &colors),
        (Look::Runner, _) => runner(&mut canvas, &spec, &colors),
        // Before the first sample the figure has nothing to show: the mark does.
        (Look::Load, _) | (Look::Number, None) => window(&mut canvas, &spec, &colors),
        (Look::Number, Some(load)) => {
            let load = load.min(100);
            let digits = load.to_string();
            let room = if spec.badge { 11.6 } else { 16. };
            let font = (if digits.len() > 2 { 8. } else { 10.5 }) * scale;
            let rect = bar_rect(size, 1.5);
            let area = [0., 0., room * scale, rect[1] - 0.5 * scale];
            if let Some(coverage) = text(&digits, area, font)
                && coverage.len() == (size * size) as usize
            {
                let tint = if load >= HIGH {
                    colors.high
                } else {
                    colors.ink
                };
                canvas.fill(tint, 1., |_, _, index| f32::from(coverage[index]) / 255.);
            }
            bar(&mut canvas, rect, Some(load), &colors);
            if spec.badge {
                let (cx, cy) = (13.2 * scale, 3. * scale);
                canvas.erase(|x, y| disc(x, y, cx, cy, 3.7 * scale));
                canvas.fill(colors.accent, 1., |x, y, _| disc(x, y, cx, cy, 2.5 * scale));
            }
        }
    }
    canvas.rgba()
}

#[cfg(test)]
mod tests {
    use super::{FRAMES, HIGH, Look, RECENT, Rect, Spec, WINDOWS, bar_rect, hex, render};

    fn none(_: &str, _: Rect, _: f32) -> Option<Vec<u8>> {
        None
    }

    fn pixel(rgba: &[u8], size: u32, x: u32, y: u32) -> [u8; 4] {
        let at = ((y * size + x) * 4) as usize;
        rgba[at..at + 4].try_into().unwrap()
    }

    fn rgb(value: u32) -> [u8; 3] {
        hex(value).map(|channel| (channel * 255.).round() as u8)
    }

    /// The same load for every recent sample.
    fn spec(look: Look, load: Option<u8>, light: bool) -> Spec {
        Spec {
            look,
            load,
            recent: [load; RECENT],
            memory: None,
            frame: 0,
            light,
            badge: false,
        }
    }

    /// Pixels of exactly `color`, fully opaque.
    fn count(rgba: &[u8], color: [u8; 3]) -> usize {
        rgba.chunks(4)
            .filter(|p| p[3] == 255 && p[..3] == color)
            .count()
    }

    #[test]
    fn every_size_the_shell_asks_for_is_filled() {
        for size in [16, 20, 24, 28, 32, 40, 48] {
            for (look, least) in [(Look::Load, 3.), (Look::Share, 16.)] {
                let rgba = render(spec(look, Some(40), false), size, &none);
                assert_eq!(rgba.len(), (size * size * 4) as usize);
                let ink = rgba.chunks(4).map(|p| f32::from(p[3]) / 255.).sum::<f32>();
                assert!(ink > (size * size) as f32 / least, "{look:?} {size}: {ink}");
            }
        }
    }

    #[test]
    fn sizes_between_the_layouts_keep_whole_pixels() {
        // 28 px: the 24 px layout, centred. 40 px: the 20 px one doubled.
        // 48 px: the 24 px one doubled. The bottom left dot (lit at 60 %) is
        // cut out on whole pixels, with the tile right up to its edges.
        for (size, left, top, side) in [(28, 7, 19, 2), (40, 6, 30, 4), (48, 10, 34, 4)] {
            let rgba = render(spec(Look::Load, Some(60), false), size, &none);
            let y = top + side / 2 - 1;
            assert_eq!(pixel(&rgba, size, left, y)[3], 0, "size {size}");
            assert_eq!(pixel(&rgba, size, left - 1, y)[3], 255, "size {size}");
            assert_eq!(pixel(&rgba, size, left + side, y)[3], 255, "size {size}");
        }
    }

    #[test]
    fn the_newest_column_follows_the_load_and_turns_amber_when_high() {
        let size = 32;
        let w = &WINDOWS[3];
        let column = w.origin.0 + (w.columns - 1) as f32 * w.pitch;
        let lit_rows = |rgba: &[u8], color: [u8; 3]| {
            (0..w.rows)
                .filter(|row| {
                    let y = w.origin.1 + *row as f32 * w.pitch;
                    pixel(rgba, size, column as u32, y as u32)[..3] == color
                        && pixel(rgba, size, column as u32, y as u32)[3] == 255
                })
                .count()
        };
        let half = render(spec(Look::Load, Some(50), false), size, &none);
        assert_eq!(lit_rows(&half, rgb(0x4cc2ff)), 3);
        // Even an idle sample lights one dot.
        let idle = render(spec(Look::Load, Some(0), false), size, &none);
        assert_eq!(lit_rows(&idle, rgb(0x4cc2ff)), 1);
        let high = render(spec(Look::Load, Some(HIGH), false), size, &none);
        assert_eq!(lit_rows(&high, rgb(0xf4b446)), 4);
        assert_eq!(count(&high, rgb(0x4cc2ff)), 0);
    }

    #[test]
    fn older_samples_are_cut_out_of_the_glyph() {
        let size = 16;
        let w = &WINDOWS[0];
        let mut s = spec(Look::Load, Some(100), true);
        s.recent = [None, None, Some(100), Some(10), Some(70), Some(100)];
        let rgba = render(s, size, &none);
        let at = |column: usize, row: usize| {
            let x = w.origin.0 + column as f32 * w.pitch;
            let y = w.origin.1 + row as f32 * w.pitch;
            pixel(&rgba, size, x as u32, y as u32)
        };
        // The first shown column is full: three holes down to the tile.
        assert!((0..3).all(|row| at(0, row)[3] == 0));
        // A light load cuts out only the bottom dot.
        assert_eq!(at(1, 2)[3], 0);
        assert_eq!(at(1, 1), [0x1a, 0x1a, 0x1a, 255]);
    }

    #[test]
    fn the_glyph_follows_the_taskbar() {
        let size = 32;
        // The title area, right of the window controls.
        for (light, ink) in [(true, rgb(0x1a1a1a)), (false, rgb(0xffffff))] {
            let p = pixel(
                &render(spec(Look::Load, None, light), size, &none),
                size,
                16,
                5,
            );
            assert_eq!(p, [ink[0], ink[1], ink[2], 255], "light={light}");
        }
    }

    #[test]
    fn the_badge_sits_in_the_title_area() {
        let size = 32;
        let with = |badge| {
            render(
                Spec {
                    badge,
                    ..spec(Look::Load, Some(30), true)
                },
                size,
                &none,
            )
        };
        let badged = with(true);
        let [cx, cy, r] = WINDOWS[3].badge;
        // The dot in the accent, a clear ring around it.
        let dot = pixel(&badged, size, cx as u32, cy as u32);
        assert_eq!(dot, [0x00, 0x67, 0xc0, 255]);
        let ring = pixel(&badged, size, (cx - r - 1.) as u32, cy as u32);
        assert!(ring[3] < 40, "{ring:?}");
        // Without the badge the tile is there.
        assert_eq!(
            pixel(&with(false), size, (cx - r - 1.) as u32, cy as u32)[3],
            255
        );
    }

    #[test]
    fn the_hundred_dots_light_as_far_as_the_load() {
        // At 16 px each of the 25 dots is 4 %: 48 % lights 12.
        let size = 16;
        let dots = |rgba: &[u8], color: [u8; 3]| count(rgba, color) / 4;
        let shown = render(spec(Look::Share, Some(48), false), size, &none);
        assert_eq!(dots(&shown, rgb(0x4cc2ff)), 12);
        let high = render(spec(Look::Share, Some(92), false), size, &none);
        assert_eq!(dots(&high, rgb(0xf4b446)), 23);
        // At 32 px the full hundred: one dot per percent.
        let full = render(spec(Look::Share, Some(37), false), 32, &none);
        assert_eq!(dots(&full, rgb(0x4cc2ff)), 37);
        // Before the first sample only the A and its ground show, nothing lit.
        let before = render(spec(Look::Share, None, false), size, &none);
        assert_eq!(dots(&before, rgb(0x4cc2ff)), 0);
        assert!(before.chunks(4).any(|p| p[3] > 0));
    }

    #[test]
    fn a_figure_is_set_from_the_text_coverage() {
        let size = 16;
        let asked = std::cell::RefCell::new(None);
        let text = |digits: &str, area: Rect, font: f32| {
            *asked.borrow_mut() = Some((digits.to_owned(), area, font));
            Some(vec![255; (size * size) as usize])
        };
        let rgba = render(spec(Look::Number, Some(7), false), size, &text);
        let (digits, area, _) = asked.borrow().clone().unwrap();
        assert_eq!(digits, "7");
        assert!(area[3] <= bar_rect(size, 1.5)[1]);
        assert_eq!(pixel(&rgba, size, 8, 2), [255, 255, 255, 255]);
        // Before the first sample there is no figure to set: the mark shows.
        let asked_before = asked.borrow().clone();
        let mark = render(spec(Look::Number, None, false), size, &text);
        assert_eq!(*asked.borrow(), asked_before);
        assert_eq!(mark, render(spec(Look::Load, None, false), size, &none));
    }

    /// The colour most of the icon's opaque pixels have.
    fn dominant(rgba: &[u8]) -> ([u8; 3], usize) {
        let mut counted = std::collections::HashMap::new();
        for pixel in rgba.chunks(4).filter(|pixel| pixel[3] == 255) {
            *counted.entry([pixel[0], pixel[1], pixel[2]]).or_insert(0) += 1;
        }
        counted
            .into_iter()
            .max_by_key(|(_, count)| *count)
            .unwrap_or(([0; 3], 0))
    }

    #[test]
    fn the_runner_moves_and_covers_the_icon_at_every_size() {
        for size in [16, 20, 24, 28, 32, 40, 48] {
            let mut seen = std::collections::HashSet::new();
            for frame in 0..FRAMES as u8 {
                let rgba = render(
                    Spec {
                        frame,
                        ..spec(Look::Runner, Some(30), false)
                    },
                    size,
                    &none,
                );
                assert_eq!(rgba.len(), (size * size * 4) as usize);
                let ink = rgba.chunks(4).map(|p| f32::from(p[3]) / 255.).sum::<f32>();
                assert!(ink > (size * size) as f32 * 0.12, "size {size}: {ink}");
                seen.insert(rgba);
            }
            // Every frame draws differently.
            assert_eq!(seen.len(), FRAMES, "size {size}");
        }
        // A frame past the last wraps onto an earlier one.
        let wrapped = render(
            Spec {
                frame: FRAMES as u8 + 2,
                ..spec(Look::Runner, Some(30), false)
            },
            32,
            &none,
        );
        let third = render(
            Spec {
                frame: 2,
                ..spec(Look::Runner, Some(30), false)
            },
            32,
            &none,
        );
        assert_eq!(wrapped, third);
    }

    #[test]
    fn the_runner_turns_amber_and_red_with_memory_use() {
        let size = 32;
        let at = |memory: u8, light: bool| {
            dominant(&render(
                Spec {
                    memory: Some(memory),
                    ..spec(Look::Runner, Some(30), light)
                },
                size,
                &none,
            ))
            .0
        };
        for light in [false, true] {
            let ink = if light { rgb(0x1a1a1a) } else { rgb(0xffffff) };
            let amber = if light { rgb(0xc27a00) } else { rgb(0xf4b446) };
            let red = if light { rgb(0xc42b1c) } else { rgb(0xff6b6b) };
            assert_eq!(at(69, light), ink, "light={light}");
            assert_eq!(at(70, light), amber, "light={light}");
            assert_eq!(at(89, light), amber, "light={light}");
            assert_eq!(at(90, light), red, "light={light}");
        }
        // Before the first sample the icon is still the cat, in the ink.
        let before = dominant(&render(spec(Look::Runner, None, false), size, &none)).0;
        assert_eq!(before, rgb(0xffffff));
    }

    #[test]
    fn the_runner_keeps_the_update_badge() {
        let size = 32;
        let plain = render(spec(Look::Runner, Some(30), true), size, &none);
        let badged = render(
            Spec {
                badge: true,
                ..spec(Look::Runner, Some(30), true)
            },
            size,
            &none,
        );
        assert_ne!(plain, badged);
        // The badge sits in the accent where the figure's does.
        assert_eq!(pixel(&badged, size, 26, 6), [0x00, 0x67, 0xc0, 255]);
        assert_ne!(pixel(&plain, size, 26, 6), [0x00, 0x67, 0xc0, 255]);
    }
}
