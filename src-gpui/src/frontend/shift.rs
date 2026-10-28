//! A visual move and fade of a laid-out subtree (`Window::with_visual_transform`):
//! layout, hit testing and element bounds stay at the final place, so a cached
//! view inside replays its last paint instead of rendering every frame.
use gpui_kit::*;

/// `child` drawn `offset` away from where it is laid out, at `opacity`.
pub(super) fn shifted(child: impl IntoElement, offset: Point<Pixels>, opacity: f32) -> Shifted {
    Shifted {
        child: child.into_any_element(),
        offset,
        opacity,
    }
}

pub(super) struct Shifted {
    child: AnyElement,
    offset: Point<Pixels>,
    opacity: f32,
}

impl IntoElement for Shifted {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Shifted {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.child.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        if self.offset == Point::default() && self.opacity >= 1. {
            self.child.paint(window, cx);
        } else {
            window.with_visual_transform(self.offset, self.opacity, |window| {
                self.child.paint(window, cx)
            });
        }
    }
}
