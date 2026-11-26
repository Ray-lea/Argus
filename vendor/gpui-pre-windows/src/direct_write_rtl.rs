//! Argus patch: the glyphs of a right-to-left run placed where DirectWrite
//! draws them, and its clusters in visual order.
use super::*;

/// Places the next glyph of a run whose pen is at `pen` and returns the x of
/// the glyph's origin, as DirectWrite draws the run. In a right-to-left run
/// (odd bidi level), given the glyph's design advance, the pen moves leftward
/// and DirectWrite sets the glyph with the right end of its design advance at
/// the pen, whatever its advance in the run; a positive advance offset moves
/// the glyph further left.
pub(super) fn place_glyph(
    pen: &mut f32,
    advance: f32,
    advance_offset: f32,
    right_to_left_design_advance: Option<f32>,
) -> f32 {
    if let Some(design_advance) = right_to_left_design_advance {
        let x = *pen - design_advance - advance_offset;
        *pen -= advance;
        x
    } else {
        let x = *pen + advance_offset;
        *pen += advance;
        x
    }
}

/// The design advances of `glyphs` in a run of `em_size`, which DirectWrite
/// sets the glyphs of a right-to-left run by (see [`place_glyph`]).
pub(super) fn design_advances(
    font_face: &IDWriteFontFace3,
    em_size: f32,
    is_sideways: BOOL,
    glyphs: &[u16],
) -> windows::core::Result<SmallVec<[f32; 32]>> {
    let mut metrics = DWRITE_FONT_METRICS1::default();
    let mut advances = SmallVec::<[i32; 32]>::from_elem(0, glyphs.len());
    unsafe {
        font_face.GetMetrics(&mut metrics);
        font_face.GetDesignGlyphAdvances(
            glyphs.len() as u32,
            glyphs.as_ptr(),
            advances.as_mut_ptr(),
            is_sideways.as_bool(),
        )?;
    }
    let scale = em_size / f32::from(metrics.Base.designUnitsPerEm);
    Ok(advances
        .into_iter()
        .map(|advance| advance as f32 * scale)
        .collect())
}

/// Reverses the clusters of a right-to-left run, whose glyphs DirectWrite lists
/// in logical order, into visual order. Each cluster keeps its own glyphs in
/// order, a base before its marks, so its first glyph stays at its left edge.
pub(super) fn clusters_in_visual_order(glyphs: Vec<ShapedGlyph>) -> Vec<ShapedGlyph> {
    let mut visual = Vec::with_capacity(glyphs.len());
    let mut end = glyphs.len();
    while end > 0 {
        let index = glyphs[end - 1].index;
        let start = glyphs[..end]
            .iter()
            .rposition(|glyph| glyph.index != index)
            .map_or(0, |previous| previous + 1);
        visual.extend_from_slice(&glyphs[start..end]);
        end = start;
    }
    visual
}
