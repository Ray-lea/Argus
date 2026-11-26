//! Argus patch: background quads whose fill follows a hover in the scene on
//! screen, without a new paint.
use super::*;

/// The background quad of an element whose hover style sets a solid
/// background and nothing else. The element's fill then changes in the scene
/// on screen as the pointer comes and goes, instead of by drawing its view
/// again (see [`crate::Window::paint_hover_background`]).
pub(crate) struct HoverQuad {
    /// Where its primitive lies in the scene's paint operations.
    pub(crate) operation: usize,
    pub(crate) link: std::rc::Rc<HoverQuadLink>,
    /// The quad as the scene holds it among its quads: moved, clipped and
    /// faded by the transforms it was painted in, and given its draw order.
    /// `None` when it is clipped out.
    pub(crate) placed: Option<Quad>,
    /// The opacity those transforms give its fill.
    pub(crate) opacity: f32,
}

/// What an element keeps of its [`HoverQuad`] from one frame to the next.
#[derive(Default)]
pub(crate) struct HoverQuadLink {
    /// Where the quad lies in the paint operations of the scene last built
    /// with it, when it was recorded there.
    pub(crate) operation: std::cell::Cell<Option<usize>>,
    /// Its fill at rest and under the pointer, with the element's opacity.
    pub(crate) fills: std::cell::Cell<(Background, Background)>,
}

impl Scene {
    /// Makes the scene another paint to its renderer after a primitive that
    /// does not animate changed in place: what it kept of the last one is
    /// compared with it again.
    pub(crate) fn mark_repainted(&mut self) {
        self.paint_revision = next_revision();
    }

    /// Inserts the background `quad` of an element whose hover changes its
    /// fill alone, and notes where it went for `link` to change the fill in
    /// place (see [`crate::Window::paint_hover_background`]).
    pub(crate) fn insert_hover_quad(&mut self, quad: Quad, link: &std::rc::Rc<HoverQuadLink>) {
        let (operation, quads) = (self.paint_operations.len(), self.quads.len());
        self.insert_primitive(quad);
        self.note_hover_quad(operation, quads, link);
    }

    /// Links `link` to the quad just recorded at `operation`, if it was;
    /// `quads` is how many quads the scene held before.
    pub(super) fn note_hover_quad(
        &mut self,
        operation: usize,
        quads: usize,
        link: &std::rc::Rc<HoverQuadLink>,
    ) {
        let recorded = self.paint_operations.len() == operation + 1
            && matches!(
                self.paint_operations[operation],
                PaintOperation::Primitive(Primitive::Quad(_))
            );
        link.operation.set(recorded.then_some(operation));
        if recorded {
            self.hover_quads.push(HoverQuad {
                operation,
                link: link.clone(),
                // Clipped out, it has no quad to show, only its operation.
                placed: (self.quads.len() == quads + 1)
                    .then(|| self.quads.last().copied())
                    .flatten(),
                opacity: self
                    .transform_stack
                    .last()
                    .map_or(1., |transform| transform.opacity),
            });
        }
    }
}
