//! The live tray icon: the Argus eye over the CPU load as a bar, or the load
//! as a figure, drawn at the size the shell asks for.
//!
//! The shapes are the design's at 16 px, scaled to the icon, and drawn as
//! signed distances with a one-pixel edge. The bar keeps whole pixels so it
//! stays sharp at every size; a figure comes from the system's text renderer
//! (the `text` callback), which returns its coverage.

/// What the icon shows besides the eye.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Look {
    /// The eye over a bar as long as the load.
    Load,
    /// The load as a figure over a thin bar.
    Number,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Spec {
    pub look: Look,
    /// CPU load in percent; none before the first sample.
    pub load: Option<u8>,
    /// The taskbar is light: dark glyphs on it.
    pub light: bool,
    /// A new version is ready.
    pub badge: bool,
}

/// The load from which the bar and the figure turn amber.
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
    badge: Rgb,
}

/// The interface's colours for each taskbar: the glyph matches the system's
/// own icons, the bar the CPU series, the badge the accent.
fn colors(light: bool) -> Colors {
    if light {
        Colors {
            ink: hex(0x1a1a1a),
            load: hex(0x0a8bbd),
            high: hex(0xc27a00),
            badge: hex(0x0067c0),
        }
    } else {
        Colors {
            ink: hex(0xffffff),
            load: hex(0x46cff4),
            high: hex(0xf4b446),
            badge: hex(0x4cc2ff),
        }
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

fn segment_distance(x: f32, y: f32, a: (f32, f32), b: (f32, f32)) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let length = dx * dx + dy * dy;
    let t = if length > 0. {
        (((x - a.0) * dx + (y - a.1) * dy) / length).clamp(0., 1.)
    } else {
        0.
    };
    ((x - a.0 - t * dx).powi(2) + (y - a.1 - t * dy).powi(2)).sqrt()
}

/// A cubic Bézier as `steps` straight pieces, `scale` pixels to the unit.
fn cubic(points: [(f32, f32); 4], scale: f32, steps: usize) -> Vec<(f32, f32)> {
    (0..=steps)
        .map(|step| {
            let t = step as f32 / steps as f32;
            let u = 1. - t;
            let weights = [u * u * u, 3. * u * u * t, 3. * u * t * t, t * t * t];
            let (mut x, mut y) = (0., 0.);
            for (weight, point) in weights.iter().zip(points) {
                x += weight * point.0;
                y += weight * point.1;
            }
            (x * scale, y * scale)
        })
        .collect()
}

/// The eye's outline, the logo's almond: two curves meeting at the corners.
/// At 16 px both lids peak on pixel centres (rows 3 and 9), so they stay sharp.
fn lids(scale: f32) -> Vec<(f32, f32)> {
    let mut points = cubic(
        [(1.2, 6.5), (3.5, 2.5), (12.5, 2.5), (14.8, 6.5)],
        scale,
        24,
    );
    points.extend(cubic(
        [(14.8, 6.5), (12.5, 10.5), (3.5, 10.5), (1.2, 6.5)],
        scale,
        24,
    ));
    points
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

/// Sets a figure in `font` pixels inside a rectangle and returns its
/// coverage, one byte per pixel of the icon, or none when it cannot.
pub(crate) type Text<'a> = dyn Fn(&str, Rect, f32) -> Option<Vec<u8>> + 'a;

/// The icon's straight RGBA pixels, `size` square.
pub(crate) fn render(spec: Spec, size: u32, text: &Text) -> Vec<u8> {
    let scale = size as f32 / 16.;
    let colors = colors(spec.light);
    let mut canvas = Canvas::new(size);
    let figure = match (spec.look, spec.load) {
        (Look::Number, Some(load)) => Some(load.min(100)),
        _ => None,
    };
    match figure {
        None => {
            let outline = lids(scale);
            let half = 1.25 * scale / 2.;
            canvas.fill(colors.ink, 1., |x, y, _| {
                let distance = outline
                    .windows(2)
                    .map(|piece| segment_distance(x, y, piece[0], piece[1]))
                    .fold(f32::MAX, f32::min);
                half + 0.5 - distance
            });
            canvas.fill(colors.ink, 1., |x, y, _| {
                disc(x, y, 8. * scale, 6.5 * scale, 2.2 * scale)
                    * (1. - disc(x, y, 8.85 * scale, 5.65 * scale, 0.65 * scale))
            });
            bar(&mut canvas, bar_rect(size, 2.5), spec.load, &colors);
        }
        Some(load) => {
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
        }
    }
    if spec.badge {
        let (cx, cy) = (13.2 * scale, 3. * scale);
        canvas.erase(|x, y| disc(x, y, cx, cy, 3.7 * scale));
        canvas.fill(colors.badge, 1., |x, y, _| disc(x, y, cx, cy, 2.5 * scale));
    }
    canvas.rgba()
}

#[cfg(test)]
mod tests {
    use super::{HIGH, Look, Rect, Spec, bar_rect, hex, render};

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

    fn spec(load: Option<u8>, light: bool) -> Spec {
        Spec {
            look: Look::Load,
            load,
            light,
            badge: false,
        }
    }

    #[test]
    fn every_size_the_shell_asks_for_is_filled() {
        for size in [16, 20, 24, 32, 40, 48] {
            let rgba = render(spec(Some(40), false), size, &none);
            assert_eq!(rgba.len(), (size * size * 4) as usize);
            // Drawn ink, in whole pixels: the thin outline is mostly edge.
            let ink = rgba.chunks(4).map(|p| f32::from(p[3]) / 255.).sum::<f32>();
            assert!(ink > (size * size) as f32 / 8., "size {size}: {ink}");
        }
    }

    #[test]
    fn the_bar_is_as_long_as_the_load_and_turns_amber_when_high() {
        let size = 32;
        let [left, top, right, bottom] = bar_rect(size, 2.5);
        let y = ((top + bottom) / 2.) as u32;
        let filled = |rgba: &[u8], color: [u8; 3]| {
            (left as u32..right as u32)
                .filter(|x| {
                    let p = pixel(rgba, size, *x, y);
                    p[3] == 255 && p[..3] == color
                })
                .count()
        };
        let width = right - left;
        let half = render(spec(Some(50), false), size, &none);
        let count = filled(&half, rgb(0x46cff4));
        assert!(
            (count as f32 - width / 2.).abs() <= 2.,
            "{count} of {width}"
        );
        assert_eq!(
            filled(&render(spec(Some(0), false), size, &none), rgb(0x46cff4)),
            0
        );
        let high = render(spec(Some(HIGH), false), size, &none);
        assert!(filled(&high, rgb(0xf4b446)) > 0);
        assert_eq!(filled(&high, rgb(0x46cff4)), 0);
        // The track stays under the empty part.
        let track = pixel(&half, size, right as u32 - 2, y);
        assert!(track[3] > 30 && track[3] < 90, "{track:?}");
    }

    #[test]
    fn the_glyph_follows_the_taskbar() {
        let size = 32;
        // The iris, left of its highlight.
        for (light, ink) in [(true, rgb(0x1a1a1a)), (false, rgb(0xffffff))] {
            let p = pixel(&render(spec(None, light), size, &none), size, 14, 14);
            assert_eq!(p, [ink[0], ink[1], ink[2], 255], "light={light}");
        }
    }

    #[test]
    fn the_badge_is_cut_out_of_the_icon() {
        let size = 32;
        let with = |badge| {
            render(
                Spec {
                    badge,
                    ..spec(Some(30), true)
                },
                size,
                &none,
            )
        };
        let badged = with(true);
        // The dot in the accent, a clear ring around it.
        let dot = pixel(&badged, size, 26, 6);
        assert_eq!(dot[..3], rgb(0x0067c0));
        assert_eq!(dot[3], 255);
        let ring = pixel(&badged, size, 26 - 6, 6 + 2);
        assert!(ring[3] < 40, "{ring:?}");
        // Without the badge the lid passes there.
        assert!(pixel(&with(false), size, 26 - 6, 6 + 2)[3] > ring[3]);
    }

    #[test]
    fn a_figure_is_set_from_the_text_coverage() {
        let size = 16;
        let asked = std::cell::RefCell::new(None);
        let text = |digits: &str, area: Rect, font: f32| {
            *asked.borrow_mut() = Some((digits.to_owned(), area, font));
            Some(vec![255; (size * size) as usize])
        };
        let rgba = render(
            Spec {
                look: Look::Number,
                ..spec(Some(7), false)
            },
            size,
            &text,
        );
        let (digits, area, _) = asked.borrow().clone().unwrap();
        assert_eq!(digits, "7");
        assert!(area[3] <= bar_rect(size, 1.5)[1]);
        assert_eq!(pixel(&rgba, size, 8, 2), [255, 255, 255, 255]);
        // Before the first sample there is no figure to set: the eye shows.
        let asked_before = asked.borrow().clone();
        render(
            Spec {
                look: Look::Number,
                ..spec(None, false)
            },
            size,
            &text,
        );
        assert_eq!(*asked.borrow(), asked_before);
    }
}
