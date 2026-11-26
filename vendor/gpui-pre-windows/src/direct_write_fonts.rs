//! Argus patch: the rasterizing mode DirectWrite recommends, and variable
//! fonts laid out at exactly the requested weight.
use super::*;

/// Argus patch: the rendering mode for a glyph run analysis, from the one
/// DirectWrite recommends for the face, size and scale. The recommendation
/// follows the font's `gasp` table: a hinted face that asks for horizontal
/// smoothing only (Microsoft YaHei UI up to 21 pixels per em) is drawn in
/// `NATURAL`, as Windows and Chromium draw it. Its hints place stroke ends on
/// whole pixels for that mode; with vertical smoothing as well
/// (`NATURAL_SYMMETRIC` on the same grid fitting) they moved some by a pixel,
/// so that the stem of 忄 in 性 stopped above the bottom stroke and 启 reached
/// a row below the characters beside it. Glyph run analysis does not support
/// `OUTLINE`, which large sizes get.
pub(super) fn rasterizing_mode(recommended: DWRITE_RENDERING_MODE1) -> DWRITE_RENDERING_MODE1 {
    match recommended {
        DWRITE_RENDERING_MODE1_OUTLINE => DWRITE_RENDERING_MODE1_NATURAL_SYMMETRIC,
        mode => mode,
    }
}

/// Argus patch: a variable font requested between its named instances is laid
/// out at exactly that weight, as browsers do, instead of at the nearest instance
/// (Segoe UI Variable has none between Regular 400 and Semibold 600).
pub(super) fn weight_axis(weight: f32) -> [DWRITE_FONT_AXIS_VALUE; 1] {
    [DWRITE_FONT_AXIS_VALUE {
        axisTag: DWRITE_FONT_AXIS_TAG_WEIGHT,
        value: weight,
    }]
}

/// The weight axis value of a variable font instance.
pub(super) fn variable_weight(font_face: &IDWriteFontFace3) -> Option<f32> {
    let face = font_face.cast::<IDWriteFontFace5>().ok()?;
    unsafe {
        if !face.HasVariations().as_bool() {
            return None;
        }
        let mut values =
            vec![DWRITE_FONT_AXIS_VALUE::default(); face.GetFontAxisValueCount() as usize];
        face.GetFontAxisValues(&mut values).ok()?;
        values
            .iter()
            .find(|value| value.axisTag == DWRITE_FONT_AXIS_TAG_WEIGHT)
            .map(|value| value.value)
    }
}
