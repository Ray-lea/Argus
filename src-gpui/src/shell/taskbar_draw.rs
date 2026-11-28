//! The taskbar monitor drawn into premultiplied BGRA for a layered window.
//! What it shows and where, in each of the four styles, along a horizontal
//! taskbar or down a vertical one, is laid out in
//! `argus_view_logic::taskbar_layout` (no GPUI), re-exported here.
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
pub(crate) use argus_view_logic::taskbar_layout::*;
use std::cell::RefCell;
use std::collections::HashMap;
use windows::Win32::Graphics::DirectWrite::*;
use windows::core::{BOOL, HSTRING, IUnknown, Interface, Ref, implement};

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
        // The monitor's opacity scales its premultiplied text and tiles in
        // step; the hover fill underneath keeps the taskbar's own strength,
        // and every pixel keeps a trace of alpha for pointer hit-testing.
        let opacity = f32::from(layout.opacity.clamp(60, 100)) / 100.;
        if opacity < 1. {
            for pixel in &mut pixels {
                for channel in pixel {
                    *channel *= opacity;
                }
            }
        }
        if hover {
            // Inset across the taskbar, as its buttons' fill, behind the
            // monitor's content.
            let mut backdrop = vec![[0f32; 4]; (width * height) as usize];
            let inset = (2. * scale).round();
            let (w, h) = (width as f32, height as f32);
            let (color, alpha) = colors.hover;
            fill(
                &mut backdrop,
                width,
                if layout.vertical {
                    [inset, 0., w - inset, h]
                } else {
                    [0., inset, w, h - inset]
                },
                4. * scale,
                color,
                alpha,
            );
            for (fg, bg) in pixels.iter_mut().zip(backdrop) {
                let clear = 1. - fg[3];
                for channel in 0..4 {
                    fg[channel] += bg[channel] * clear;
                }
            }
        }
        let mut bytes = Vec::with_capacity(pixels.len() * 4);
        for [r, g, b, a] in pixels {
            let byte = |v: f32| (v.clamp(0., 1.) * 255. + 0.5) as u8;
            bytes.extend([byte(b), byte(g), byte(r), byte(a).max(1)]);
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
    use super::{Cell, Fonts, Group, Look, Painter, Run, Tone, lay_out};
    use crate::settings::TaskbarStyle as Style;

    fn look(style: Style) -> Look {
        Look {
            style,
            ..Look::default()
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
                ..Cell::default()
            }],
            ..Group::default()
        }];
        let layout = lay_out(&groups, 48, 1.25, look(Style::Labels), &painter, None);
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
        // Lower opacity dims premultiplied content while keeping both the
        // 1/255 hit-test floor and the taskbar's native hover fill intact.
        let dim = lay_out(
            &groups,
            48,
            1.25,
            Look {
                opacity: 60,
                ..look(Style::Labels)
            },
            &painter,
            None,
        );
        let faded = painter.paint(&dim, false, false, 1.25).unwrap();
        let faded_at = |x: i32, y: i32| {
            let index = ((y * dim.width + x) * 4) as usize;
            [
                faded[index],
                faded[index + 1],
                faded[index + 2],
                faded[index + 3],
            ]
        };
        let bright = {
            let right = dim.runs[1].x + painter.width(&dim.runs[1].text, dim.runs[1].size, true);
            (dim.runs[1].baseline - dim.runs[1].size..dim.runs[1].baseline + 3)
                .flat_map(|y| (dim.runs[1].x..right).map(move |x| (x, y)))
                .map(|(x, y)| faded_at(x, y))
                .max_by_key(|pixel| pixel[3])
                .unwrap()
        };
        assert!(bright[3] > 60 && bright[3] < value[3], "{bright:?}");
        assert_eq!(faded_at(0, 0), [0, 0, 0, 1]);
        let hover_opaque = painter.paint(&layout, false, true, 1.25).unwrap();
        let hover_dim = painter.paint(&dim, false, true, 1.25).unwrap();
        let sample = ((10 * layout.width + 2) * 4) as usize;
        assert_eq!(
            &hover_opaque[sample..sample + 4],
            &hover_dim[sample..sample + 4]
        );
    }

    /// Readings as the taskbar shows them, for looking at the text and the
    /// styles (`ARGUS_TASKBAR_SAMPLES=<png path> cargo test -p argus --lib
    /// taskbar_samples -- --ignored`): each style on a dark and a light
    /// taskbar at 100 %, Windows 11's 48 px high and then Windows 10's
    /// 40 px, and beside it the same four times as large.
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
                ..Cell::default()
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
        let painter = Painter::new(15).unwrap();
        let mut strips = Vec::new();
        for (height, style) in [48, 40].into_iter().flat_map(|height| {
            [
                Style::Labels,
                Style::Curves,
                Style::Capsules,
                Style::Figures,
            ]
            .map(|style| (height, style))
        }) {
            let layout = lay_out(&groups, height, 1., look(style), &painter, None);
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
