//! Argus patch: decorative images and bars whose bounds change in a clean
//! presented scene without a new paint.
use super::*;

/// A decorative primitive whose layout and hit boxes remain fixed. Handles are
/// valid only until the next full window draw; callers redraw on a failed update.
#[derive(Clone, Copy)]
pub struct PaintedPrimitive {
    window_id: WindowId,
    generation: u64,
    operation: usize,
    layer: Bounds<ScaledPixels>,
    image: Option<(crate::ImageId, usize)>,
    background: Option<Background>,
    // A fully clipped image has no scene primitive. Keep its effective device
    // mask so invisible translations can still complete without a new paint.
    culled_mask: Option<Bounds<ScaledPixels>>,
}
impl PaintedPrimitive {
    /// Whether the last full paint found this image entirely outside its clip.
    pub fn is_clipped_out(&self) -> bool {
        self.culled_mask.is_some()
    }
    fn find_index(&self, scene: &Scene) -> Option<usize> {
        use crate::scene::{PaintOperation, Primitive};
        match scene.paint_operations.get(self.operation)? {
            PaintOperation::Primitive(Primitive::PolychromeSprite(value)) => {
                let mut hits = scene
                    .polychrome_sprites
                    .iter()
                    .enumerate()
                    .filter(|(_, sprite)| {
                        sprite.order == value.order
                            && sprite.tile == value.tile
                            && sprite.bounds == value.bounds
                            && sprite.content_mask.bounds == value.content_mask.bounds
                    });
                let first = hits.next()?.0;
                hits.next().is_none().then_some(first)
            }
            PaintOperation::Primitive(Primitive::Quad(value)) => {
                let mut hits = scene.quads.iter().enumerate().filter(|(_, quad)| {
                    quad.order == value.order
                        && quad.bounds == value.bounds
                        && quad.content_mask.bounds == value.content_mask.bounds
                });
                let first = hits.next()?.0;
                hits.next().is_none().then_some(first)
            }
            _ => None,
        }
    }
}

impl Window {
    /// Device bounds and fill of an animated bar. Its sides and baseline snap to
    /// device pixels; its top keeps a fractional position, drawn as a coverage
    /// ramp one device pixel tall, so the bar grows smoothly between pixel rows
    /// instead of jumping a whole row at a time. None when nothing would show.
    ///
    /// The quad starts half a pixel above the top, where the ramp begins, rather
    /// than at the pixel row holding the top: its rounded corners then move with
    /// the top instead of jumping a whole row each time the top crosses one. It
    /// never starts above `ceiling` (the top of the bar's reserved region).
    fn retained_bar(
        &self,
        bounds: Bounds<Pixels>,
        color: Hsla,
        ceiling: ScaledPixels,
    ) -> crate::RetainedBar {
        let scale_factor = self.scale_factor();
        let left = round_to_device_pixel(bounds.left().0, scale_factor);
        let right = round_to_device_pixel(bounds.right().0, scale_factor).max(left);
        let bottom = round_to_device_pixel(bounds.bottom().0, scale_factor);
        let top = (bounds.top().0 * scale_factor).min(bottom);
        crate::RetainedBar {
            bounds: Bounds::from_corners(
                point(ScaledPixels(left), ScaledPixels(top)),
                point(ScaledPixels(right), ScaledPixels(bottom)),
            ),
            ceiling,
            color,
        }
    }

    /// Paint a complete image at fractional device coordinates, clipped by the
    /// current content mask. Animated raster layers must retain their subpixel
    /// position instead of snapping bounds or rounding a cropped atlas tile.
    /// The Windows renderer area-filters these images (see
    /// [`PolychromeSprite::AREA_FILTER`]), so a raster drawn at a multiple of the
    /// device resolution keeps the same antialiasing at every position.
    pub fn paint_image_subpixel(
        &mut self,
        bounds: Bounds<Pixels>,
        data: Arc<RenderImage>,
        frame_index: usize,
    ) -> Result<Option<PaintedPrimitive>> {
        self.paint_filtered_image(bounds, data, frame_index, PolychromeSprite::AREA_FILTER)
    }

    /// Paint a premultiplied BGRA image with subpixel area filtering on Windows.
    /// Use a separate RenderImage from any straight-alpha representation.
    pub fn paint_image_subpixel_premultiplied(
        &mut self,
        bounds: Bounds<Pixels>,
        data: Arc<RenderImage>,
        frame_index: usize,
    ) -> Result<Option<PaintedPrimitive>> {
        self.paint_filtered_image(
            bounds,
            data,
            frame_index,
            PolychromeSprite::PREMULTIPLIED_AREA_FILTER,
        )
    }

    /// Paint a premultiplied image whose displayed pixels must include its backdrop.
    /// Retained bounds can still move without repainting its parent element.
    pub fn paint_image_subpixel_in_scene(
        &mut self,
        bounds: Bounds<Pixels>,
        data: Arc<RenderImage>,
        frame_index: usize,
    ) -> Result<Option<PaintedPrimitive>> {
        self.paint_filtered_image(
            bounds,
            data,
            frame_index,
            PolychromeSprite::IN_SCENE_AREA_FILTER,
        )
    }

    fn paint_filtered_image(
        &mut self,
        bounds: Bounds<Pixels>,
        data: Arc<RenderImage>,
        frame_index: usize,
        filter: u32,
    ) -> Result<Option<PaintedPrimitive>> {
        self.invalidator.debug_assert_paint();
        let params = RenderImageParams {
            image_id: data.id,
            frame_index,
        };
        let tile = self
            .sprite_atlas
            .get_or_insert_with(params.into(), &mut || {
                Ok(Some((
                    data.size(frame_index),
                    Cow::Borrowed(data.as_bytes(frame_index).expect("valid image frame")),
                )))
            })?
            .expect("image tile");
        let content_mask = self.snapped_content_mask();
        let sprite = PolychromeSprite {
            order: 0,
            pad: filter,
            grayscale: false.into(),
            bounds: bounds.scale(self.scale_factor()),
            content_mask,
            corner_radii: Corners::default(),
            tile,
            opacity: self.element_opacity(),
        };
        // Reserve the full clip region so subsequent translations cannot change
        // this image's ordering relative to other primitives.
        let region = self.content_mask().bounds;
        let layer = self.cover_bounds(region);
        let handle = self.paint_layer(region, |window| {
            let operation = window.next_frame.scene.len();
            window.next_frame.scene.insert_primitive(sprite);
            let culled_mask =
                (window.next_frame.scene.len() == operation).then_some(content_mask.bounds);
            PaintedPrimitive {
                window_id: window.handle.window_id(),
                generation: window.paint_generation,
                operation,
                layer,
                image: Some((data.id, frame_index)),
                background: None,
                culled_mask,
            }
        });
        Ok(Some(handle))
    }

    /// Paint an antialiased solid rectangle, with optionally rounded corners, in
    /// a fixed animation region. Empty rectangles retain an invisible primitive,
    /// allowing a bar to grow again without changing scene ordering or
    /// introducing a nonzero reading.
    pub fn paint_animated_rect(
        &mut self,
        bounds: Bounds<Pixels>,
        region: Bounds<Pixels>,
        color: Hsla,
        corner_radius: Pixels,
    ) -> Option<PaintedPrimitive> {
        use crate::scene::{PaintOperation, Primitive};
        let layer = self.cover_bounds(region.intersect(&self.content_mask().bounds));
        // The fill as painted, with the element opacity applied.
        let retained =
            self.retained_bar(bounds, color.opacity(self.element_opacity()), layer.top());
        let bar = retained.paint();
        self.paint_layer(region, |window| {
            let operation = window.next_frame.scene.len();
            window.paint_quad(crate::fill(region, color).corner_radii(corner_radius));
            if window.next_frame.scene.len() != operation + 1 {
                return None;
            }
            let scene = &mut window.next_frame.scene;
            let Some(PaintOperation::Primitive(Primitive::Quad(recorded))) =
                scene.paint_operations.get_mut(operation)
            else {
                return None;
            };
            let background = recorded.background;
            let quad = scene.quads.last_mut()?;
            // A retained bar stays inside its reserved animation region.
            // Keep its restoration mask equally tight so an enclosing plot
            // clip cannot force unrelated charts back through background copies.
            quad.content_mask.bounds = quad.content_mask.bounds.intersect(&layer);
            match bar {
                Some((bounds, fill)) => {
                    quad.bounds = bounds;
                    quad.background = fill;
                }
                None => quad.background = background.opacity(0.),
            }
            *recorded = *quad;
            scene.retained_bars.insert(scene.quads.len() - 1, retained);
            Some(PaintedPrimitive {
                window_id: window.handle.window_id(),
                generation: window.paint_generation,
                operation,
                layer,
                image: None,
                background: Some(background),
                culled_mask: None,
            })
        })
    }

    /// Request a complete paint and rebuild the platform's cached drawing.
    pub fn invalidate_render_cache(&mut self) {
        self.force_full_render = true;
        self.refresh();
    }

    /// Update a decorative image translation or bar extent in an otherwise clean
    /// scene from an `on_next_frame` callback. Dirty views, expired handles,
    /// incompatible image resizes and movement outside the reserved region require a full redraw.
    pub fn update_painted_bounds(
        &mut self,
        handle: &PaintedPrimitive,
        bounds: Bounds<Pixels>,
    ) -> bool {
        use crate::scene::{PaintOperation, Primitive};
        if handle.window_id != self.handle.window_id()
            || handle.generation != self.paint_generation
            || self.invalidator.is_dirty()
        {
            return false;
        }
        if ![
            bounds.left(),
            bounds.top(),
            bounds.size.width,
            bounds.size.height,
        ]
        .iter()
        .all(|value| f32::from(*value).is_finite())
            || bounds.size.width < px(0.)
            || bounds.size.height < px(0.)
        {
            return false;
        }
        let tile = if let Some((image_id, frame_index)) = handle.image {
            let params = RenderImageParams {
                image_id,
                frame_index,
            };
            let Ok(Some(tile)) = self
                .sprite_atlas
                .get_or_insert_with(params.into(), &mut || Ok(None))
            else {
                return false;
            };
            Some(tile)
        } else {
            None
        };
        let requested = bounds.scale(self.scale_factor());
        if let Some(mask) = handle.culled_mask {
            // Entering the visible region needs a full paint to create the
            // primitive and its order. Remaining outside changes no pixels.
            return requested.intersect(&mask).is_empty();
        }
        let scene = &self.rendered_frame.scene;
        let Some(index) = *self
            .painted_primitive_indices
            .entry(handle.operation)
            .or_insert_with(|| handle.find_index(scene))
        else {
            return false;
        };
        let bar = match (tile, handle.background.and_then(|b| b.as_solid())) {
            (None, Some(color)) => Some(self.retained_bar(bounds, color, handle.layer.top())),
            _ => None,
        };
        let scene = &mut self.rendered_frame.scene;
        let Some(PaintOperation::Primitive(recorded)) =
            scene.paint_operations.get_mut(handle.operation)
        else {
            return false;
        };
        let fits = |bounds: Bounds<ScaledPixels>, mask: Bounds<ScaledPixels>| {
            let clipped = bounds.intersect(&mask);
            !clipped.is_empty()
                && clipped.left() >= handle.layer.left()
                && clipped.top() >= handle.layer.top()
                && clipped.right() <= handle.layer.right()
                && clipped.bottom() <= handle.layer.bottom()
        };
        let changed = match (recorded, tile) {
            (Primitive::PolychromeSprite(recorded), Some(tile)) => {
                let Some(sprite) = scene.polychrome_sprites.get_mut(index) else {
                    return false;
                };
                if sprite.tile != tile
                    || recorded.tile != tile
                    || (sprite.bounds.size != requested.size
                        && !(sprite.pad == PolychromeSprite::IN_SCENE_AREA_FILTER
                            && sprite.bounds.size.width == requested.size.width))
                    || !fits(requested, sprite.content_mask.bounds)
                {
                    return false;
                }
                let changed = sprite.bounds != requested;
                sprite.bounds = requested;
                recorded.bounds = requested;
                changed
            }
            (Primitive::Quad(recorded), None) => {
                let Some(quad) = scene.quads.get_mut(index) else {
                    return false;
                };
                let Some(background) = handle.background else {
                    return false;
                };
                let Some(bar) = bar else {
                    return false;
                };
                let (target, fill) = bar
                    .paint()
                    .unwrap_or((handle.layer, background.opacity(0.)));
                if !fits(target, quad.content_mask.bounds) {
                    return false;
                }
                scene.retained_bars.insert(index, bar);
                let changed = quad.bounds != target || quad.background != fill;
                quad.bounds = target;
                quad.background = fill;
                *recorded = *quad;
                changed
            }
            _ => return false,
        };
        let order = match &scene.paint_operations[handle.operation] {
            PaintOperation::Primitive(Primitive::PolychromeSprite(sprite)) => sprite.order,
            PaintOperation::Primitive(Primitive::Quad(quad)) => quad.order,
            _ => return false,
        };
        scene.animated_order = Some(scene.animated_order.map_or(order, |old| old.min(order)));
        if changed {
            self.needs_present.set(true);
        }
        true
    }
}
