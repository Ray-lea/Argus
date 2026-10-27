//! Shared native controls and short, reduced-motion-aware transitions.
use super::*;
use gpui_kit::component::tab::{Tab, TabBar};

impl Desktop {
    pub(in crate::frontend) fn segments(
        &self,
        id: impl Into<SharedString>,
        choices: Vec<(String, Option<&str>, Action, bool)>,
        cx: &Context<Self>,
    ) -> AnyElement {
        let selected = choices.iter().position(|choice| choice.3).unwrap_or(0);
        let actions = choices
            .iter()
            .map(|choice| choice.2.clone())
            .collect::<Vec<_>>();
        TabBar::new(id.into())
            .segmented()
            .selected_index(selected)
            .children(choices.into_iter().map(|(name, glyph, action, _)| {
                let icon_only = name.is_empty();
                let accessible = if icon_only && let Action::View(mode) = &action {
                    label(&format!("shell.viewMode.{mode}.label"))
                } else {
                    name.clone()
                };
                Tab::new()
                    .label(name)
                    .aria_label(accessible)
                    .when_some(glyph, |tab, glyph| {
                        if icon_only {
                            tab.icon(icon(glyph, self.palette.muted))
                        } else {
                            tab.prefix(icon(glyph, self.palette.muted))
                        }
                    })
            }))
            .on_click(cx.listener(move |this, index: &usize, window, cx| {
                this.act(actions[*index].clone(), window, cx);
            }))
            .into_any_element()
    }

    pub(in crate::frontend) fn switch(
        &self,
        id: impl Into<SharedString>,
        name: impl Into<SharedString>,
        value: bool,
        action: Action,
        cx: &Context<Self>,
    ) -> AnyElement {
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(div().text_color(self.palette.muted).child(label(if value {
                "settings.toggle.on"
            } else {
                "settings.toggle.off"
            })))
            .child(
                gpui_kit::component::switch::Switch::new(id.into())
                    .checked(value)
                    .accessibility_label(name)
                    .on_change(cx.listener(move |this, _, window, cx| {
                        this.act(action.clone(), window, cx);
                    })),
            )
            .into_any_element()
    }

    /// The new page slides in from the side of its navigation entry: from the right
    /// for a tab further right, from below for an entry further down the pane.
    pub(in crate::frontend) fn enter(
        &self,
        id: impl Into<ElementId>,
        content: impl IntoElement,
        cx: &App,
    ) -> AnyElement {
        let element = div()
            .relative()
            .size_full()
            .min_h_0()
            .min_w_0()
            .flex()
            .flex_col()
            .child(content);
        if cx.reduce_motion() || self.page_direction == 0. {
            self.page_opacity.set(1.);
            return element.into_any_element();
        }
        let opacity = self.page_opacity.clone();
        let (dx, dy) = if self.nav_top {
            (self.page_direction * 32., 0.)
        } else {
            (0., self.page_direction * 24.)
        };
        let ease = gpui_kit::base::animation::cubic_bezier(0.1, 0.9, 0.2, 1.);
        element
            .with_animation(
                id,
                Animation::new(std::time::Duration::from_millis(300)).with_easing(ease),
                move |element, progress| {
                    opacity.set(progress);
                    element
                        .opacity(progress)
                        .left(px((1. - progress) * dx))
                        .top(px((1. - progress) * dy))
                },
            )
            .into_any_element()
    }
}
