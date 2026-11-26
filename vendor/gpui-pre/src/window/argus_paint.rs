//! Argus patch: painting outside layout (visual and live transforms and
//! clips, the composition overlay, rules, moving regions) and measured
//! layout with a baseline.
use super::*;

impl Window {
    /// Argus: tells the renderer that the content painted inside `bounds`
    /// (within the current content mask) is drawn `offset` away from where it
    /// was laid out, as a scroll container or a sliding page draws it, under
    /// an `id` that stays the same while it moves. A renderer may copy what
    /// only moved by whole device pixels since its last frame, after checking
    /// each primitive (`ScrollRegion`).
    pub fn report_moving_region(
        &mut self,
        id: usize,
        bounds: Bounds<Pixels>,
        offset: Point<Pixels>,
    ) {
        let scale = self.scale_factor();
        let clip = bounds.intersect(&self.content_mask().bounds);
        if clip.size.width > Pixels::ZERO && clip.size.height > Pixels::ZERO {
            self.next_frame
                .scene
                .scroll_regions
                .push(crate::ScrollRegion {
                    id,
                    clip: clip.scale(scale),
                    offset: point(offset.x.scale(scale), offset.y.scale(scale)),
                });
        }
    }

    /// Paints `f` moved by `offset` and faded by `opacity` without changing
    /// layout, element bounds or hit testing (transforms nest). Cached views
    /// painted inside replay their last paint at the new place and opacity, so
    /// moving a cached subtree costs no rendering. Call during paint.
    pub fn with_visual_transform<R>(
        &mut self,
        offset: Point<Pixels>,
        opacity: f32,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        self.invalidator.debug_assert_paint();
        let transform =
            crate::VisualTransform::moved(offset.scale(self.scale_factor()), opacity.clamp(0., 1.));
        self.next_frame.scene.push_transform(transform);
        let result = f(self);
        self.next_frame.scene.pop_transform();
        result
    }

    /// Paints `f` inside `transform` about `anchor` (window coordinates): like
    /// [`Self::with_visual_transform`], layout, element bounds and hit testing
    /// stay as laid out. The transform's value is read whenever the scene is
    /// built, so a cached view painted inside (or around) it that replays its
    /// last paint draws with the value set since: an animation can change the
    /// value every frame without rendering the views it moves. Call during paint.
    pub fn with_live_transform<R>(
        &mut self,
        transform: &crate::LiveTransform,
        anchor: Point<Pixels>,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        self.invalidator.debug_assert_paint();
        let scale_factor = self.scale_factor();
        self.next_frame.scene.push_live_transform(
            transform.clone(),
            anchor.scale(scale_factor),
            scale_factor,
        );
        let result = f(self);
        self.next_frame.scene.pop_transform();
        result
    }

    /// Paints `f` clipped to `bounds` without changing content masks, layout
    /// or hit testing (clips nest, with transforms too). Unlike an overflow
    /// clip, a cached view painted inside replays its last paint under a new
    /// clip instead of rendering again. Call during paint.
    pub fn with_visual_clip<R>(
        &mut self,
        bounds: Bounds<Pixels>,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        self.invalidator.debug_assert_paint();
        let transform = crate::VisualTransform {
            clip: Some(bounds.scale(self.scale_factor())),
            ..crate::VisualTransform::moved(Point::default(), 1.)
        };
        self.next_frame.scene.push_transform(transform);
        let result = f(self);
        self.next_frame.scene.pop_transform();
        result
    }

    /// Whether this window can present an overlay on its own composition surface.
    pub fn supports_composition_overlay(&self) -> bool {
        self.platform_window.supports_composition_overlay()
    }

    /// Begin a full-window overlay. A compositing backend can keep the already
    /// painted content in one surface and the overlay and later paint in another.
    pub fn paint_composition_overlay<R>(
        &mut self,
        bounds: Bounds<Pixels>,
        draw: impl FnOnce(&mut Self) -> R,
    ) -> R {
        if bounds.intersect(&self.content_mask().bounds).is_empty() {
            return draw(self);
        }
        self.paint_layer(bounds, |window| {
            window.next_frame.scene.mark_composition_overlay();
            draw(window)
        })
    }

    /// Paint a solid axis-aligned rule. Its short side is a stroke width, snapped
    /// independently of its position, including when a cached scene moves or scales.
    pub fn paint_rule(&mut self, axis: crate::Axis, bounds: Bounds<Pixels>, color: Hsla) {
        self.invalidator.debug_assert_paint();
        if bounds.is_empty() {
            return;
        }
        let mut snapped = self.snap_bounds(bounds);
        match axis {
            crate::Axis::Horizontal => snapped.size.height = self.snap_stroke(bounds.size.height),
            crate::Axis::Vertical => snapped.size.width = self.snap_stroke(bounds.size.width),
        }
        let quad = Quad {
            bounds: snapped,
            content_mask: self.snapped_content_mask(),
            background: color.opacity(self.element_opacity()).into(),
            ..Default::default()
        };
        self.next_frame.scene.insert_rule(axis, quad);
    }

    /// As [`Window::request_measured_layout`], for an element that also reports how far its
    /// first baseline lies below its top edge, as text does. Containers aligning their items to
    /// baselines (`items_baseline`) align that baseline; an element without one counts as having
    /// its baseline at its bottom edge.
    ///
    /// This method should only be called as part of the request_layout or prepaint phase of element drawing.
    pub fn request_measured_layout_with_baseline<F>(&mut self, style: Style, measure: F) -> LayoutId
    where
        F: Fn(
                Size<Option<Pixels>>,
                Size<AvailableSpace>,
                &mut Window,
                &mut App,
            ) -> (Size<Pixels>, Option<Pixels>)
            + 'static,
    {
        self.invalidator.debug_assert_prepaint();

        let rem_size = self.rem_size();
        let scale_factor = self.scale_factor();
        self.layout_engine
            .as_mut()
            .unwrap()
            .request_measured_layout_with_baseline(style, rem_size, scale_factor, measure)
    }
}
