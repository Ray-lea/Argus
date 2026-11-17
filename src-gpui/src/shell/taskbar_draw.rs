//! What the taskbar monitor shows: short readings in columns of two rows
//! (one on a taskbar too low for two), each a name on the left and its value
//! on the right, drawn into premultiplied BGRA for a layered window.
//!
//! A column is as wide as its widest reading may get (`Cell::widest`), so
//! that changing values neither move the columns nor resize the window.
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
use windows::Win32::Graphics::DirectWrite::*;
use windows::core::{BOOL, HSTRING, IUnknown, Interface, Ref, implement};

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

/// The text size at `scale`, pixels: the interface's own (`resources/theme.json`
/// `fontSize`). Two rows of it fit a 48 px taskbar; at the clock's 12 px the
/// readings were harder to make out.
pub(crate) fn font_size(scale: f32) -> i32 {
    (13. * scale).round().max(8.) as i32
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
            name: 0.72,
            hover: (hex(0x000000), 0.05),
        }
    } else {
        Colors {
            ink: hex(0xffffff),
            high: hex(0xf4b446),
            name: 0.78,
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
/// `width` × `height`. The factory, the text formats and the display's text
/// rendering parameters stay for the next image of the same text size.
pub(crate) struct Painter {
    factory: IDWriteFactory3,
    renderer: IDWriteTextRenderer,
    names: IDWriteTextFormat,
    values: IDWriteTextFormat,
    typography: IDWriteTypography,
    /// The baseline below a row's top, centring the capitals in the row.
    baseline: f32,
    /// The display's alpha correction, as the interface applies it.
    gamma: [f32; 4],
    contrast: f32,
    /// How the three samples of a pixel make one coverage.
    weights: [u16; 3],
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
            let locale = HSTRING::from(argus_core::i18n::locale().tag());
            let format = |weight: f32| -> Option<IDWriteTextFormat> {
                let format = factory
                    .CreateTextFormat(
                        &family,
                        &collection,
                        DWRITE_FONT_WEIGHT(weight as i32),
                        DWRITE_FONT_STYLE_NORMAL,
                        DWRITE_FONT_STRETCH_NORMAL,
                        font as f32,
                        &locale,
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
                Some(format)
            };
            let typography = factory.CreateTypography().ok()?;
            typography
                .AddFontFeature(DWRITE_FONT_FEATURE {
                    nameTag: DWRITE_FONT_FEATURE_TAG_TABULAR_FIGURES,
                    parameter: 1,
                })
                .ok()?;
            // The capitals' middle on the row's middle.
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
            let cap = f32::from(metrics.capHeight) / f32::from(metrics.designUnitsPerEm.max(1))
                * font as f32;
            let line = (font as f32 * 4. / 3.).round();
            Some(Self {
                renderer: Renderer.into(),
                names: format(NAME_WEIGHT)?,
                values: format(value_weight)?,
                typography,
                baseline: ((line + cap) / 2.).round(),
                gamma: gpui_kit::gpui::get_gamma_correction_ratios(params.GetGamma()),
                contrast: params.GetGrayscaleEnhancedContrast(),
                weights,
                factory,
                font,
            })
        }
    }

    /// `text` laid out on one line, in a value's weight or a name's.
    fn text(&self, text: &str, value: bool) -> Option<IDWriteTextLayout> {
        let wide: Vec<u16> = text.encode_utf16().collect();
        unsafe {
            let layout = self
                .factory
                .CreateTextLayout(
                    &wide,
                    if value { &self.values } else { &self.names },
                    f32::INFINITY,
                    f32::INFINITY,
                )
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

    /// The width of `text` in pixels, in a value's weight or a name's.
    pub fn width(&self, text: &str, value: bool) -> i32 {
        if text.is_empty() {
            return 0;
        }
        let mut metrics = DWRITE_TEXT_METRICS::default();
        self.text(text, value)
            .and_then(|layout| unsafe { layout.GetMetrics(&mut metrics) }.ok())
            .map_or(0, |_| {
                metrics.widthIncludingTrailingWhitespace.ceil() as i32
            })
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
            let rect = [0., inset, width as f32, height as f32 - inset];
            let radius = 4. * scale;
            let (color, alpha) = colors.hover;
            for (index, pixel) in pixels.iter_mut().enumerate() {
                let x = (index as i32 % width) as f32 + 0.5;
                let y = (index as i32 / width) as f32 + 0.5;
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
            let Some(text) = self.text(&run.text, run.value) else {
                continue;
            };
            let mut target = Target {
                painter: self,
                pixels: &mut pixels,
                width,
                height,
                baseline: run.rect[1] as f32 + self.baseline,
                color,
                alpha,
            };
            unsafe {
                let _ = text.Draw(
                    Some((&raw mut target).cast::<std::ffi::c_void>().cast_const()),
                    &self.renderer,
                    run.rect[0] as f32,
                    run.rect[1] as f32,
                );
            }
        }
        let mut bytes = Vec::with_capacity(pixels.len() * 4);
        for [r, g, b, a] in pixels {
            let byte = |v: f32| (v.clamp(0., 1.) * 255. + 0.5) as u8;
            bytes.extend([byte(b), byte(g), byte(r), byte(a)]);
        }
        Some(bytes)
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
        assert_eq!(layout.font, 13);
        // Two rows of 17 px in the middle of 48.
        let values: Vec<_> = layout.runs.iter().filter(|run| run.value).collect();
        assert_eq!(values[0].rect, [8 + 76 - 20, 7, 84, 24]);
        assert_eq!(values[1].rect, [8 + 76 - 30, 24, 84, 41]);
        // The same readings with other values keep the size.
        let busier: Vec<Group> = vec![vec![cell("CPU", "100%")], vec![cell("MEM", "99%")]];
        assert_eq!(lay_out(&busier, 48, 1., measure).width, layout.width);
        // A taskbar too low for two rows sets them side by side.
        let low = lay_out(&groups, 30, 1., measure);
        assert_eq!(low.width, 8 + 76 + 14 + 76 + 8);
        assert!(low.runs.iter().all(|run| run.rect[1] == 6));
        // At 150 % everything grows with the scale.
        assert_eq!(font_size(1.5), 20);
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

    /// Readings as the taskbar shows them, for looking at the text
    /// (`ARGUS_TASKBAR_SAMPLES=<png path> cargo test -p argus --lib
    /// taskbar_samples -- --ignored`): each size and value weight on a dark
    /// and a light taskbar, at 100 %.
    #[test]
    #[ignore]
    fn taskbar_samples() {
        let Ok(path) = std::env::var("ARGUS_TASKBAR_SAMPLES") else {
            return;
        };
        let cell = |label: &str, value: &str, widest: &str, high: bool| Cell {
            label: label.into(),
            value: value.into(),
            widest: widest.into(),
            high,
        };
        let groups: Vec<Group> = vec![
            vec![cell("CPU", "41%", "100%", false)],
            vec![cell("CPU", "63 °C", "100 °C", false)],
            vec![cell("内存", "41%", "100%", false)],
            vec![cell("GPU", "88%", "100%", true)],
            vec![cell("GPU", "52 °C", "100 °C", false)],
            vec![cell("显存", "38%", "100%", false)],
            vec![
                cell("读", "757 KB/s", "999.9 MB/s", false),
                cell("写", "16.0 MB/s", "999.9 MB/s", false),
            ],
            vec![
                cell("↓", "585.9 Mbps", "999.9 Mbps", false),
                cell("↑", "20.7 Mbps", "999.9 Mbps", false),
            ],
        ];
        let mut strips = Vec::new();
        for font in [12, 13] {
            for weight in [400., 500., 600.] {
                let painter = Painter::with_weight(font, weight).unwrap();
                let layout = lay_out(&groups, 48, 1., |text, value| painter.width(text, value));
                for (light, background) in [(false, [32u8, 32, 32]), (true, [238, 238, 238])] {
                    let image = painter.paint(&layout, light, false, 1.).unwrap();
                    strips.push((layout.width, layout.height, image, background));
                }
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
        // Its first columns enlarged four times, pixel for pixel.
        let (zoom, part) = (4, width.min(236) as usize);
        let mut large = Vec::with_capacity(part * height as usize * 4 * zoom * zoom);
        for y in 0..(height as usize * zoom) {
            for x in 0..(part * zoom) {
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
            (part * zoom) as u32,
            height * zoom as u32,
            &path.replace(".png", "-4x.png"),
        );
    }
}
