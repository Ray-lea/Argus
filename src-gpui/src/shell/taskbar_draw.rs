//! What the taskbar monitor shows: short readings in columns of two rows
//! (one on a taskbar too low for two), each a name on the left and its value
//! on the right, set by the system's text renderer and drawn into
//! premultiplied BGRA for a layered window.
//!
//! A column is as wide as its widest reading may get (`Cell::widest`), so
//! that changing values neither move the columns nor resize the window. The
//! text comes from GDI as grey-scale coverage, drawn white on black and then
//! tinted per run, as the tray icon's figure.
use windows::Win32::Foundation::{COLORREF, RECT, SIZE};
use windows::Win32::Graphics::Gdi::*;

/// One reading on the taskbar.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Cell {
    /// Its short name: "CPU", or an arrow for a rate.
    pub label: String,
    pub value: String,
    /// The widest value it takes, so that its column keeps its width.
    pub widest: String,
    /// High enough to show amber.
    pub high: bool,
}

/// What one metric shows: one cell, or the two of a pair of rates (disk,
/// network), which keep to one column.
pub(crate) type Group = Vec<Cell>;

/// Arranges the groups in columns of `rows` cells: a group's cells stay
/// together, and single cells share a column in order.
pub(crate) fn columns(groups: &[Group], rows: usize) -> Vec<Vec<&Cell>> {
    let mut columns: Vec<Vec<&Cell>> = Vec::new();
    // The column a single cell opened, which the next single cell fills.
    let mut open: Option<usize> = None;
    for group in groups {
        if rows < 2 {
            columns.extend(group.iter().map(|cell| vec![cell]));
            continue;
        }
        match group.as_slice() {
            [] => {}
            [cell] => match open.take() {
                Some(index) => columns[index].push(cell),
                None => {
                    open = Some(columns.len());
                    columns.push(vec![cell]);
                }
            },
            cells => columns.extend(cells.chunks(rows).map(|chunk| chunk.iter().collect())),
        }
    }
    columns
}

/// Where a text goes, in pixels of the image.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Run {
    pub text: String,
    /// Left, top, right, bottom: as wide as the text, as high as its row.
    pub rect: [i32; 4],
    /// Set in the value's weight and colour rather than the name's.
    pub value: bool,
    pub high: bool,
}

/// The monitor laid out at one size.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Layout {
    pub width: i32,
    pub height: i32,
    pub runs: Vec<Run>,
    /// The size of the text, pixels.
    pub font: i32,
}

/// The text size at `scale`, pixels: as the taskbar's clock.
pub(crate) fn font_size(scale: f32) -> i32 {
    (12. * scale).round().max(8.) as i32
}

/// Lays out `groups` in a window `height` pixels high at `scale`;
/// `measure(text, value)` gives a text's width in pixels.
pub(crate) fn lay_out(
    groups: &[Group],
    height: i32,
    scale: f32,
    measure: impl Fn(&str, bool) -> i32,
) -> Layout {
    let font = font_size(scale);
    let px = |logical: f32| (logical * scale).round() as i32;
    let line = (font as f32 * 4. / 3.).round() as i32;
    let rows = if height >= 2 * line + px(4.) { 2 } else { 1 };
    let (pad, inner, gap) = (px(8.), px(6.), px(14.));
    let mut runs = Vec::new();
    let mut x = pad;
    let columns = columns(groups, rows);
    for (index, column) in columns.iter().enumerate() {
        if index > 0 {
            x += gap;
        }
        let names = column
            .iter()
            .map(|cell| measure(&cell.label, false))
            .collect::<Vec<_>>();
        let values = column
            .iter()
            .map(|cell| measure(&cell.value, true))
            .collect::<Vec<_>>();
        let width = column
            .iter()
            .zip(&names)
            .zip(&values)
            .map(|((cell, name), value)| {
                let room = (*value).max(measure(&cell.widest, true));
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
        for (row, ((cell, name), value)) in column.iter().zip(&names).zip(&values).enumerate() {
            let (y, bottom) = (top + row as i32 * line, top + (row as i32 + 1) * line);
            if !cell.label.is_empty() {
                runs.push(Run {
                    text: cell.label.clone(),
                    rect: [x, y, x + name, bottom],
                    value: false,
                    high: false,
                });
            }
            runs.push(Run {
                text: cell.value.clone(),
                rect: [x + width - value, y, x + width, bottom],
                value: true,
                high: cell.high,
            });
        }
        x += width;
    }
    Layout {
        width: if columns.is_empty() { 0 } else { x + pad },
        height,
        runs,
        font,
    }
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
/// step quieter than values, amber for high values, and the fill a taskbar
/// button shows under the pointer.
struct Colors {
    ink: Rgb,
    high: Rgb,
    name: f32,
    hover: (Rgb, f32),
}

fn colors(light: bool) -> Colors {
    if light {
        Colors {
            ink: hex(0x1a1a1a),
            high: hex(0xc27a00),
            name: 0.68,
            hover: (hex(0x000000), 0.05),
        }
    } else {
        Colors {
            ink: hex(0xffffff),
            high: hex(0xf4b446),
            name: 0.72,
            hover: (hex(0xffffff), 0.08),
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

/// Draws `text` coverage into GDI and composes the image: premultiplied
/// BGRA, `width` × `height`. The text's fonts and memory DC stay for the
/// next image of the same text size.
pub(crate) struct Painter {
    dc: HDC,
    regular: HFONT,
    bold: HFONT,
    pub font: i32,
}

impl Painter {
    pub fn new(font: i32) -> Option<Self> {
        unsafe {
            let dc = CreateCompatibleDC(None);
            if dc.is_invalid() {
                return None;
            }
            let make = |weight: i32| {
                let mut name = [0u16; 32];
                for (to, from) in name.iter_mut().zip("Segoe UI".encode_utf16()) {
                    *to = from;
                }
                CreateFontIndirectW(&LOGFONTW {
                    lfHeight: -font.max(1),
                    lfWeight: weight,
                    lfCharSet: DEFAULT_CHARSET,
                    lfQuality: ANTIALIASED_QUALITY,
                    lfFaceName: name,
                    ..Default::default()
                })
            };
            Some(Self {
                dc,
                regular: make(FW_NORMAL.0 as i32),
                bold: make(FW_SEMIBOLD.0 as i32),
                font,
            })
        }
    }

    /// The width of `text` in pixels, in a value's weight or a name's.
    pub fn width(&self, text: &str, value: bool) -> i32 {
        if text.is_empty() {
            return 0;
        }
        let text: Vec<u16> = text.encode_utf16().collect();
        let mut size = SIZE::default();
        unsafe {
            let old = SelectObject(self.dc, if value { self.bold } else { self.regular }.into());
            let _ = GetTextExtentPoint32W(self.dc, &text, &mut size);
            SelectObject(self.dc, old);
        }
        size.cx
    }

    /// The image of `layout` on a light or dark taskbar, with the pointer's
    /// fill when `hover`. Every pixel keeps a trace of alpha so that the
    /// whole window answers the pointer, not only its glyphs.
    pub fn paint(&self, layout: &Layout, light: bool, hover: bool, scale: f32) -> Option<Vec<u8>> {
        let (width, height) = (layout.width, layout.height);
        if width <= 0 || height <= 0 {
            return None;
        }
        let coverage = self.coverage(layout)?;
        let colors = colors(light);
        let mut pixels = vec![[0f32; 4]; (width * height) as usize];
        let floor = 1. / 255.;
        for pixel in &mut pixels {
            pixel[3] = floor;
        }
        let blend = |pixel: &mut [f32; 4], color: Rgb, a: f32| {
            if a <= 0. {
                return;
            }
            for channel in 0..3 {
                pixel[channel] = color[channel] * a + pixel[channel] * (1. - a);
            }
            pixel[3] = a + pixel[3] * (1. - a);
        };
        if hover {
            let inset = (2. * scale).round();
            let rect = [0., inset, width as f32, height as f32 - inset];
            let radius = 4. * scale;
            for (index, pixel) in pixels.iter_mut().enumerate() {
                let x = (index as i32 % width) as f32 + 0.5;
                let y = (index as i32 / width) as f32 + 0.5;
                let (color, alpha) = colors.hover;
                blend(pixel, color, alpha * rounded(x, y, rect, radius));
            }
        }
        for run in &layout.runs {
            let (color, alpha) = if !run.value {
                (colors.ink, colors.name)
            } else if run.high {
                (colors.high, 1.)
            } else {
                (colors.ink, 1.)
            };
            // A glyph may overhang its advance by a pixel.
            let [left, top, right, bottom] = run.rect;
            for y in top.max(0)..bottom.min(height) {
                for x in (left - 1).max(0)..(right + 1).min(width) {
                    let index = (y * width + x) as usize;
                    let a = coverage[index] as f32 / 255. * alpha;
                    blend(&mut pixels[index], color, a);
                }
            }
        }
        let mut bytes = Vec::with_capacity(pixels.len() * 4);
        for [r, g, b, a] in pixels {
            let byte = |v: f32| (v.clamp(0., 1.) * 255.).round() as u8;
            bytes.extend([byte(b), byte(g), byte(r), byte(a)]);
        }
        Some(bytes)
    }

    /// Each run's text drawn white on black, one byte of coverage per pixel.
    fn coverage(&self, layout: &Layout) -> Option<Vec<u8>> {
        let (width, height) = (layout.width, layout.height);
        unsafe {
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width,
                    biHeight: -height,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits = std::ptr::null_mut();
            let bitmap =
                CreateDIBSection(Some(self.dc), &info, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
            if bits.is_null() {
                let _ = DeleteObject(bitmap.into());
                return None;
            }
            let old_bitmap = SelectObject(self.dc, bitmap.into());
            let old_font = SelectObject(self.dc, self.regular.into());
            SetBkMode(self.dc, TRANSPARENT);
            SetTextColor(self.dc, COLORREF(0x00ff_ffff));
            for run in &layout.runs {
                SelectObject(
                    self.dc,
                    if run.value { self.bold } else { self.regular }.into(),
                );
                let [left, top, right, bottom] = run.rect;
                let mut area = RECT {
                    left,
                    top,
                    right,
                    bottom,
                };
                let mut text: Vec<u16> = run.text.encode_utf16().collect();
                DrawTextW(
                    self.dc,
                    &mut text,
                    &mut area,
                    DT_SINGLELINE
                        | DT_VCENTER
                        | DT_NOPREFIX
                        | DT_NOCLIP
                        | if run.value { DT_RIGHT } else { DT_LEFT },
                );
            }
            let _ = GdiFlush();
            let coverage =
                std::slice::from_raw_parts(bits.cast::<u8>(), (width * height * 4) as usize)
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|pixel| pixel[1])
                    .collect();
            SelectObject(self.dc, old_font);
            SelectObject(self.dc, old_bitmap);
            let _ = DeleteObject(bitmap.into());
            Some(coverage)
        }
    }
}

impl Drop for Painter {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(self.regular.into());
            let _ = DeleteObject(self.bold.into());
            let _ = DeleteDC(self.dc);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Cell, Group, Painter, columns, font_size, lay_out};

    fn cell(label: &str, value: &str) -> Cell {
        Cell {
            label: label.into(),
            value: value.into(),
            widest: "100%".into(),
            high: false,
        }
    }

    fn names(columns: &[Vec<&Cell>]) -> Vec<Vec<String>> {
        columns
            .iter()
            .map(|column| column.iter().map(|cell| cell.label.clone()).collect())
            .collect()
    }

    #[test]
    fn single_readings_pair_up_and_rates_keep_their_own_column() {
        let groups: Vec<Group> = vec![
            vec![cell("CPU", "1%")],
            vec![cell("↓", "1 KB/s"), cell("↑", "2 KB/s")],
            vec![cell("MEM", "3%")],
            vec![cell("GPU", "4%")],
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
    fn columns_keep_the_width_of_their_widest_value_and_rows_centre() {
        // Every character 10 px wide, at 100 %.
        let measure = |text: &str, _: bool| text.chars().count() as i32 * 10;
        let groups: Vec<Group> = vec![vec![cell("CPU", "1%")], vec![cell("MEM", "45%")]];
        let layout = lay_out(&groups, 48, 1., measure);
        // Padding 8, name 30, gap 6, the widest value "100%" 40, padding 8.
        assert_eq!(layout.width, 8 + 30 + 6 + 40 + 8);
        assert_eq!(layout.font, 12);
        // Two rows of 16 px in the middle of 48.
        let values: Vec<_> = layout.runs.iter().filter(|run| run.value).collect();
        assert_eq!(values[0].rect, [8 + 76 - 20, 8, 84, 24]);
        assert_eq!(values[1].rect, [8 + 76 - 30, 24, 84, 40]);
        // The same readings with other values keep the size.
        let busier: Vec<Group> = vec![vec![cell("CPU", "100%")], vec![cell("MEM", "99%")]];
        assert_eq!(lay_out(&busier, 48, 1., measure).width, layout.width);
        // A taskbar too low for two rows sets them side by side.
        let low = lay_out(&groups, 30, 1., measure);
        assert_eq!(low.width, 8 + 76 + 14 + 76 + 8);
        assert!(low.runs.iter().all(|run| run.rect[1] == 7));
        // At 150 % everything grows with the scale.
        assert_eq!(font_size(1.5), 18);
    }

    #[test]
    fn text_shows_in_its_runs_and_the_rest_keeps_a_trace_of_alpha() {
        let Some(painter) = Painter::new(16) else {
            return;
        };
        let groups: Vec<Group> = vec![vec![Cell {
            label: "CPU".into(),
            value: "88%".into(),
            widest: "100%".into(),
            high: true,
        }]];
        let layout = lay_out(&groups, 48, 1.25, |text, value| painter.width(text, value));
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
        let strongest = |rect: [i32; 4]| {
            (rect[1]..rect[3])
                .flat_map(|y| (rect[0]..rect[2]).map(move |x| (x, y)))
                .map(|(x, y)| at(x, y))
                .max_by_key(|pixel| pixel[3])
                .unwrap()
        };
        let value = strongest(layout.runs[1].rect);
        assert!(value[3] > 200 && value[2] > value[0], "{value:?}");
        let name = strongest(layout.runs[0].rect);
        assert!(name[3] > 120 && name[0] == name[2], "{name:?}");
    }
}
