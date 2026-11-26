//! The static prefix and the animated suffix of a scene: where its batches
//! split, what the suffix draws over, and whether the suffix's images can
//! compose the cached prefix behind themselves.
use super::*;

pub(super) fn split_batches(scene: &Scene, boundary: Option<DrawOrder>) -> Vec<PrimitiveBatch> {
    split_primitive_batches(scene, scene.batches(), boundary)
}

pub(super) fn split_primitive_batches(
    scene: &Scene,
    source: impl Iterator<Item = PrimitiveBatch>,
    boundary: Option<DrawOrder>,
) -> Vec<PrimitiveBatch> {
    let Some(boundary) = boundary else {
        return source.collect();
    };
    let mut batches = Vec::new();
    for batch in source {
        macro_rules! split {
            ($items:expr, $range:expr, $make:expr) => {{
                let range = $range;
                let middle = range.start
                    + $items[range.clone()].partition_point(|item| item.order < boundary);
                if middle > range.start {
                    batches.push(($make)(range.start..middle));
                }
                if middle < range.end {
                    batches.push(($make)(middle..range.end));
                }
            }};
        }
        match batch {
            PrimitiveBatch::Shadows(range) => split!(scene.shadows, range, PrimitiveBatch::Shadows),
            PrimitiveBatch::Quads(range) => split!(scene.quads, range, PrimitiveBatch::Quads),
            PrimitiveBatch::Paths(range) => split!(scene.paths, range, PrimitiveBatch::Paths),
            PrimitiveBatch::Underlines(range) => {
                split!(scene.underlines, range, PrimitiveBatch::Underlines)
            }
            PrimitiveBatch::Surfaces(range) => {
                split!(scene.surfaces, range, PrimitiveBatch::Surfaces)
            }
            PrimitiveBatch::MonochromeSprites { texture_id, range } => {
                split!(scene.monochrome_sprites, range, |range| {
                    PrimitiveBatch::MonochromeSprites { texture_id, range }
                })
            }
            PrimitiveBatch::SubpixelSprites { texture_id, range } => {
                split!(scene.subpixel_sprites, range, |range| {
                    PrimitiveBatch::SubpixelSprites { texture_id, range }
                })
            }
            PrimitiveBatch::PolychromeSprites { texture_id, range } => {
                split!(scene.polychrome_sprites, range, |range| {
                    PrimitiveBatch::PolychromeSprites { texture_id, range }
                })
            }
        }
    }
    batches
}

pub(super) fn suffix_damage(
    scene: &Scene,
    batches: &[PrimitiveBatch],
    width: u32,
    height: u32,
) -> Option<Vec<RECT>> {
    let mut regions = Vec::new();
    let mut include = |bounds: Bounds<ScaledPixels>, mask: ContentMask<ScaledPixels>| {
        let clipped = bounds.intersect(&mask.bounds);
        if !clipped.is_empty() {
            let rect = RECT {
                left: (clipped.left().as_f32().floor() as i32 - 1).clamp(0, width as i32),
                top: (clipped.top().as_f32().floor() as i32 - 1).clamp(0, height as i32),
                right: (clipped.right().as_f32().ceil() as i32 + 1).clamp(0, width as i32),
                bottom: (clipped.bottom().as_f32().ceil() as i32 + 1).clamp(0, height as i32),
            };
            if rect.right > rect.left && rect.bottom > rect.top {
                merge_damage(&mut regions, rect);
            }
        }
    };
    for batch in batches {
        match batch {
            // Shadows can extend beyond primitive bounds; use full presentation.
            PrimitiveBatch::Shadows(_) | PrimitiveBatch::Surfaces(_) => return None,
            PrimitiveBatch::Quads(range) => {
                for item in &scene.quads[range.clone()] {
                    // Animated bars can shrink, including to zero. Restore the
                    // whole mask so pixels from previous frames are erased.
                    include(item.content_mask.bounds, item.content_mask);
                }
            }
            PrimitiveBatch::Paths(range) => {
                for item in &scene.paths[range.clone()] {
                    include(item.bounds, item.content_mask);
                }
            }
            PrimitiveBatch::Underlines(range) => {
                for item in &scene.underlines[range.clone()] {
                    include(item.bounds, item.content_mask);
                }
            }
            PrimitiveBatch::MonochromeSprites { range, .. } => {
                for item in &scene.monochrome_sprites[range.clone()] {
                    include(item.bounds, item.content_mask);
                }
            }
            PrimitiveBatch::SubpixelSprites { range, .. } => {
                for item in &scene.subpixel_sprites[range.clone()] {
                    include(item.bounds, item.content_mask);
                }
            }
            PrimitiveBatch::PolychromeSprites { range, .. } => {
                for item in &scene.polychrome_sprites[range.clone()] {
                    include(item.content_mask.bounds, item.content_mask);
                }
            }
        }
    }
    (!regions.is_empty()).then_some(regions)
}

pub(super) fn primitive_count(scene: &Scene) -> usize {
    scene.quads.len()
        + scene.underlines.len()
        + scene.monochrome_sprites.len()
        + scene.subpixel_sprites.len()
        + scene.polychrome_sprites.len()
}

pub(super) fn batch_last_order(scene: &Scene, batch: &PrimitiveBatch) -> DrawOrder {
    match batch {
        PrimitiveBatch::Shadows(range) => scene.shadows[range.end - 1].order,
        PrimitiveBatch::Quads(range) => scene.quads[range.end - 1].order,
        PrimitiveBatch::Paths(range) => scene.paths[range.end - 1].order,
        PrimitiveBatch::Underlines(range) => scene.underlines[range.end - 1].order,
        PrimitiveBatch::MonochromeSprites { range, .. } => {
            scene.monochrome_sprites[range.end - 1].order
        }
        PrimitiveBatch::SubpixelSprites { range, .. } => {
            scene.subpixel_sprites[range.end - 1].order
        }
        PrimitiveBatch::PolychromeSprites { range, .. } => {
            scene.polychrome_sprites[range.end - 1].order
        }
        PrimitiveBatch::Surfaces(range) => scene.surfaces[range.end - 1].order,
    }
}

// A single sprite can restore its own background while painting. Only disjoint
// masks are eligible: overlapping layers must retain the ordinary blend order.
pub(super) struct PrefixComposition {
    /// Everything the composed suffix writes. Frames present what changed
    /// instead, which covers this whenever the suffix changed.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) dirty: Vec<RECT>,
    pub(super) restore: Vec<RECT>,
}

fn composition_rejected(reason: &'static str) -> Option<PrefixComposition> {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    if *ENABLED.get_or_init(|| std::env::var_os("ARGUS_GPUI_DIAGNOSTICS").is_some()) {
        static COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let count = COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if count < 3 || count.is_multiple_of(400) {
            println!("{{\"event\":\"gpu-composition-reject\",\"reason\":\"{reason}\"}}");
        }
    }
    None
}

pub(super) fn subtract_rect(rect: RECT, cover: RECT) -> impl Iterator<Item = RECT> {
    let left = rect.left.max(cover.left);
    let top = rect.top.max(cover.top);
    let right = rect.right.min(cover.right);
    let bottom = rect.bottom.min(cover.bottom);
    let pieces = if left >= right || top >= bottom {
        [rect, RECT::default(), RECT::default(), RECT::default()]
    } else {
        [
            RECT {
                bottom: top,
                ..rect
            },
            RECT {
                top: bottom,
                ..rect
            },
            RECT {
                top,
                bottom,
                right: left,
                ..rect
            },
            RECT {
                top,
                bottom,
                left: right,
                ..rect
            },
        ]
    };
    pieces
        .into_iter()
        .filter(|piece| piece.left < piece.right && piece.top < piece.bottom)
}

pub(super) fn independent_sprite_regions(
    scene: &Scene,
    batches: &[PrimitiveBatch],
    width: u32,
    height: u32,
) -> Option<PrefixComposition> {
    let mut regions: Vec<RECT> = Vec::new();
    let mut quads = Vec::new();
    let mut images = Vec::new();
    for batch in batches {
        if let PrimitiveBatch::Quads(range) = batch {
            quads.push(PrimitiveBatch::Quads(range.clone()));
            continue;
        }
        let PrimitiveBatch::PolychromeSprites { range, .. } = batch else {
            return composition_rejected("unsupported-batch");
        };
        for sprite in &scene.polychrome_sprites[range.clone()] {
            if !sprite.bounds.size.width.as_f32().is_finite()
                || !sprite.bounds.size.height.as_f32().is_finite()
                || sprite.bounds.size.width.as_f32() <= 0.
                || sprite.bounds.size.height.as_f32() <= 0.
            {
                return composition_rejected("invalid-bounds");
            }
            let mask = sprite.content_mask.bounds;
            let rect = RECT {
                left: (mask.left().as_f32().floor() as i32).clamp(0, width as i32),
                top: (mask.top().as_f32().floor() as i32).clamp(0, height as i32),
                right: (mask.right().as_f32().ceil() as i32).clamp(0, width as i32),
                bottom: (mask.bottom().as_f32().ceil() as i32).clamp(0, height as i32),
            };
            if rect.left >= rect.right || rect.top >= rect.bottom {
                continue;
            }
            // The composed vertex shader covers the complete mask; fragments
            // outside the image return the cached background. A rounded mask
            // may extend a pixel beyond an image at fractional display scales.
            if regions.iter().any(|other| {
                rect.left < other.right
                    && other.left < rect.right
                    && rect.top < other.bottom
                    && other.top < rect.bottom
            }) {
                return composition_rejected("overlapping-images");
            }
            regions.push(rect);
            images.push((rect, sprite.order));
        }
    }
    if regions.is_empty() {
        return composition_rejected("empty-regions");
    }
    let mut restore = Vec::new();
    for batch in quads {
        let PrimitiveBatch::Quads(range) = batch else {
            unreachable!()
        };
        for index in range {
            let Some(damage) = suffix_damage(
                scene,
                &[PrimitiveBatch::Quads(index..index + 1)],
                width,
                height,
            ) else {
                continue;
            };
            // Earlier images restore their own masks before this bar is drawn.
            // Copy only the bar's remaining background, including when its
            // restoration padding crosses the edge of a chart.
            let mut uncovered = damage;
            for (image, order) in &images {
                if *order < scene.quads[index].order
                    && uncovered.iter().any(|rect| {
                        rect.left < image.right
                            && image.left < rect.right
                            && rect.top < image.bottom
                            && image.top < rect.bottom
                    })
                {
                    uncovered = uncovered
                        .into_iter()
                        .flat_map(|rect| subtract_rect(rect, *image))
                        .collect();
                }
            }
            for rect in uncovered {
                merge_damage(&mut restore, rect);
            }
        }
    }
    if restore.iter().any(|quad| {
        regions.iter().any(|image| {
            quad.left < image.right
                && image.left < quad.right
                && quad.top < image.bottom
                && image.top < quad.bottom
        })
    }) {
        return composition_rejected("quad-restore-overlaps-image");
    }
    regions.extend_from_slice(&restore);
    Some(PrefixComposition {
        dirty: regions,
        restore,
    })
}
