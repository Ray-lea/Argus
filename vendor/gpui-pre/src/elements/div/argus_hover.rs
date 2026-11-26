//! Argus patch: the fills an element takes in place when its hover style
//! sets a solid background alone.
use super::*;

impl Interactivity {
    /// Argus patch: the two fills of this element when its hover style sets a
    /// solid background and nothing else, for its background to follow the
    /// pointer in place (`Window::paint_hover_background`). `style` is the
    /// style just computed for painting. Nothing may lie over the hover style:
    /// a press or a drag styles the element after it, and then the element is
    /// painted as before.
    pub(super) fn hover_background(
        &self,
        hitbox: Option<&Hitbox>,
        element_state: Option<&mut InteractiveElementState>,
        style: &Style,
        in_place: bool,
        cx: &App,
    ) -> Option<crate::style::HoverBackground> {
        let rest = self.rest_background.take();
        let element_state = element_state?;
        // Whatever follows, the quad of an earlier frame is not this one's.
        if let Some(link) = &element_state.hover_quad {
            link.operation.set(None);
        }
        // Only where an element around this one allows it
        // (`hover_fills_in_place`): elsewhere a hover draws the view again.
        if !in_place {
            return None;
        }
        hitbox?;
        let hover = self.hover_style.as_ref()?.background_alone()?;
        // The hover listener keeps its state here; without it nothing follows.
        element_state.hover_state.as_ref()?;
        if cx.has_active_drag()
            || element_state
                .clicked_state
                .as_ref()
                .is_some_and(|state| state.borrow().is_clicked())
        {
            return None;
        }
        let (hovered, base) = match rest {
            Some(base) => (true, base),
            None => (false, style.background.clone()),
        };
        let base = match base {
            Some(fill) => {
                let background = fill.color()?;
                background.as_solid()?;
                Some(background)
            }
            None => None,
        };
        let link = element_state
            .hover_quad
            .get_or_insert_with(Default::default)
            .clone();
        Some(crate::style::HoverBackground {
            link,
            base,
            hover,
            hovered,
        })
    }
}
