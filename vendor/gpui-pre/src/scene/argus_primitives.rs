//! Argus patch: the scroll regions a scene reports, the geometry of retained
//! bars and the area-filter modes of polychrome sprites.
use super::*;

/// Argus: a scroll container of a scene. A renderer can compare its offset
/// with the last scene's: content that moved by whole device pixels inside
/// `clip` may be copied there, after checking that the primitives agree.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollRegion {
    /// Stable while the container keeps its scroll state.
    pub id: usize,
    /// Where the container shows its content, in device pixels.
    pub clip: Bounds<ScaledPixels>,
    /// How far its content is scrolled, in device pixels.
    pub offset: Point<ScaledPixels>,
}

/// Physical geometry shared by the normal shader and compositor bar cache.
#[derive(Clone, Copy, Debug)]
pub struct RetainedBar {
    /// Pixel-aligned sides and bottom, with a fractional top.
    pub bounds: Bounds<ScaledPixels>,
    /// Top of the reserved track.
    pub ceiling: ScaledPixels,
    /// Full-strength bar color.
    pub color: Hsla,
}

impl RetainedBar {
    /// Geometry and coverage ramp for the antialiased leading edge.
    pub fn paint(self) -> Option<(Bounds<ScaledPixels>, Background)> {
        let Self {
            bounds,
            ceiling,
            color,
        } = self;
        let top = bounds.top().0.min(bounds.bottom().0);
        if bounds.size.width.0 <= 0. || bounds.bottom().0 - top < 1. / 256. {
            return None;
        }
        let start = (top - 0.5).max(ceiling.0.min(top + 1e-3).floor());
        let height = bounds.bottom().0 - start;
        let fill = crate::linear_gradient(
            180.,
            crate::linear_color_stop(color.opacity(0.), (top - 0.5 - start) / height),
            crate::linear_color_stop(color, (top + 0.5 - start) / height),
        )
        .coverage_ramp();
        Some((
            Bounds::from_corners(
                point(bounds.left(), ScaledPixels(start)),
                bounds.bottom_right(),
            ),
            fill,
        ))
    }
}

impl PolychromeSprite {
    /// Sample the image by the area each pixel covers, with premultiplied
    /// texels, instead of bilinearly: for rasters drawn at fractional positions
    /// from more texels than pixels (Windows renderer).
    pub const AREA_FILTER: u32 = 1;

    /// Area-filter an image whose RGB channels are already multiplied by alpha.
    /// The Windows renderer combines adjacent texels with hardware linear sampling.
    pub const PREMULTIPLIED_AREA_FILTER: u32 = 2;

    /// Blend the filtered image with the scene before presentation. A platform
    /// may move cached pixels if they already include their opaque backdrop.
    pub const IN_SCENE_AREA_FILTER: u32 = 3;
}
