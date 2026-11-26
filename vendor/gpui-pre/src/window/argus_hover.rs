//! Argus patch: hover fills that change in the scene on screen, without
//! drawing the element's view again.
use super::*;

/// The border colour [`Style::paint`] gives a background quad: the fill
/// without its alpha, so that the quad's antialiased edge fades to nothing
/// rather than to black.
fn hover_edge_color(background: Background) -> Hsla {
    let mut color = background.solid;
    color.a = 0.;
    color
}

impl Window {
    /// Paints the background of an element whose hover style sets a solid
    /// background and nothing else, at `bounds`, as a quad that takes either
    /// fill in place. A row or a button under a moving pointer changes fill
    /// far more often than anything else about it changes: drawing its view
    /// again for each change builds, lays out and paints everything in the
    /// view to change one quad. With no background at rest the quad is still
    /// there, transparent, so that hovering has something to fill.
    ///
    /// The quad is what [`Self::paint_quad`] makes of the same fill, and
    /// [`Self::set_hover_fill`] changes it later.
    pub(crate) fn paint_hover_background(
        &mut self,
        bounds: Bounds<Pixels>,
        corner_radii: Corners<Pixels>,
        border_style: BorderStyle,
        hover: &crate::style::HoverBackground,
    ) {
        self.invalidator.debug_assert_paint();
        let opacity = self.element_opacity();
        let fills = (
            hover.base.unwrap_or_default().opacity(opacity),
            hover.hover.opacity(opacity),
        );
        hover.link.fills.set(fills);
        let background = if hover.hovered { fills.1 } else { fills.0 };
        let quad = Quad {
            order: 0,
            bounds: self.snap_bounds(bounds),
            content_mask: self.snapped_content_mask(),
            background,
            border_color: hover_edge_color(background),
            corner_radii: corner_radii.scale(self.scale_factor()),
            border_widths: Edges::default(),
            border_style,
        };
        self.next_frame.scene.insert_hover_quad(quad, &hover.link);
    }

    /// Gives the quad that [`Self::paint_hover_background`] painted for `link`
    /// the fill for `hovered`, in the scene on screen, and presents it: no
    /// view draws again. Returns `false` when the quad is not in that scene as
    /// it was painted (its view was drawn inside a transform, or another quad
    /// equals it); the caller then draws the element's view again.
    ///
    /// The paint operation changes with the quad, so a later frame that
    /// replays the view shows the same fill.
    pub(crate) fn set_hover_fill(
        &mut self,
        link: &crate::scene::HoverQuadLink,
        hovered: bool,
    ) -> bool {
        use crate::scene::{PaintOperation, Primitive};
        if !self.invalidator.not_drawing() {
            return false;
        }
        let Some(operation) = link.operation.get() else {
            return false;
        };
        let (base, hover) = link.fills.get();
        let background = if hovered { hover } else { base };
        let scene = &mut self.rendered_frame.scene;
        // The scene on screen is the one the link was last noted in.
        let Some(entry) = scene
            .hover_quads
            .binary_search_by_key(&operation, |quad| quad.operation)
            .ok()
            .filter(|index| std::ptr::eq(Rc::as_ptr(&scene.hover_quads[*index].link), link))
        else {
            return false;
        };
        let Some(PaintOperation::Primitive(Primitive::Quad(recorded))) =
            scene.paint_operations.get(operation)
        else {
            return false;
        };
        if recorded.background == background {
            return true;
        }
        // The quad among the scene's quads, as the transforms around the
        // element placed it. Clipped out, there is none to show.
        let placed = scene.hover_quads[entry].placed;
        let opacity = scene.hover_quads[entry].opacity;
        let index = match placed {
            Some(placed) => {
                let quads = &scene.quads;
                let Some(index) = *self
                    .painted_primitive_indices
                    .entry(operation)
                    .or_insert_with(|| {
                        let mut hits = quads.iter().enumerate().filter(|(_, quad)| {
                            quad.order == placed.order
                                && quad.bounds == placed.bounds
                                && quad.content_mask.bounds == placed.content_mask.bounds
                                && quad.background == placed.background
                        });
                        let first = hits.next()?.0;
                        hits.next().is_none().then_some(first)
                    })
                else {
                    return false;
                };
                if scene.quads.get(index).is_none_or(|quad| {
                    quad.bounds != placed.bounds || quad.background != placed.background
                }) {
                    return false;
                }
                Some(index)
            }
            None => None,
        };
        if let Some(PaintOperation::Primitive(Primitive::Quad(recorded))) =
            scene.paint_operations.get_mut(operation)
        {
            recorded.background = background;
            recorded.border_color = hover_edge_color(background);
        }
        let Some(index) = index else {
            return true;
        };
        let shown = background.opacity(opacity);
        let quad = &mut scene.quads[index];
        quad.background = shown;
        quad.border_color = hover_edge_color(shown);
        scene.hover_quads[entry].placed = Some(*quad);
        scene.mark_repainted();
        self.needs_present.set(true);
        self.invalidator.wake_platform();
        true
    }
}
