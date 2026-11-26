//! Argus patch: text reported to assistive technology, and the complete
//! frames that send it the tree a frame replaying cached views lacks.
use super::*;

impl Window {
    /// Argus patch: draws a complete frame, which sends assistive technology
    /// the tree that a frame replaying cached views could not build: right
    /// away after a pause, and at most once per `REPLAY_REFRESH_INTERVAL`
    /// while something animates. A refresh renders every view instead of
    /// replaying it (a focus change refreshes already).
    pub(super) fn refresh_for_a11y(&mut self, cx: &mut App) {
        if self.a11y.refresh_pending {
            return;
        }
        self.a11y.refresh_pending = true;
        let delay = self.a11y.refresh_delay();
        self.spawn(cx, async move |cx| {
            cx.background_executor.timer(delay).await;
            cx.update(|window, _| {
                window.a11y.refresh_pending = false;
                window.refresh();
            })
            .ok();
        })
        .detach();
    }

    /// Argus patch: reports text that an element without an element id
    /// draws in `bounds` to assistive technology, while it prepaints. GPUI's
    /// own text elements (strings, [`StyledText`]) call this; an element
    /// that lays out and draws text itself should too. Depending on the
    /// nearest accessibility node around it, the text names, describes or
    /// gives the value of that node, or becomes a label of its own (see
    /// `A11y::report_text`). Does nothing while no assistive technology
    /// reads the window, or inside [`Self::with_a11y_text_hidden`].
    pub fn report_a11y_text(&mut self, text: &str, bounds: Bounds<Pixels>) {
        if !self.a11y.is_active() {
            return;
        }
        let scope = self.element_id_stack.hash();
        let scale_factor = self.scale_factor();
        let _node = self.a11y.report_text(scope, text, bounds, scale_factor);
        #[cfg(debug_assertions)]
        if let Some(node_id) = _node {
            let view = self.a11y.view_type_names.get(&self.current_view()).copied();
            self.a11y.nodes.record_node_info(
                node_id,
                crate::window::a11y::debug::NodeDebugInfo {
                    synthetic: true,
                    view,
                    element_id: None,
                    source_location: None,
                },
            );
        }
    }

    /// Argus patch: acts as if assistive technology had started reading the
    /// window, which the test platform has no adapter for, and draws a
    /// complete frame as the activation does.
    #[cfg(any(test, feature = "test-support"))]
    pub fn activate_a11y_for_test(&mut self) {
        self.a11y.activate_for_test();
        self.refresh();
    }

    /// Argus patch: text reported while `f` prepaints is not reported if
    /// `hidden`: it lies in a box that is not painted, as one with
    /// `visibility: hidden` (which `div` does here itself) is not.
    pub fn with_a11y_text_hidden<R>(&mut self, hidden: bool, f: impl FnOnce(&mut Self) -> R) -> R {
        if !hidden || !self.a11y.is_active() {
            return f(self);
        }
        self.a11y.hidden_text += 1;
        let result = f(self);
        self.a11y.hidden_text -= 1;
        result
    }
}
