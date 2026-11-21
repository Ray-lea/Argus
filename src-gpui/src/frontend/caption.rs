//! The title row's end: the window's caption buttons at the size Windows
//! gives its own, 46 × 32 px and flush with the window's top-right corner,
//! and before them the button that keeps the window above other windows.
//!
//! Minimize, maximize and close are native caption areas
//! (`WindowControlArea`): Windows hit-tests them as its own buttons, so the
//! maximize button keeps its snap layouts flyout, and their clicks go
//! through the platform window rather than through GPUI. Keeping on top is
//! an ordinary control of the client area.
use super::*;
use gpui_kit::component::{ActiveTheme, GlobalState, Icon, IconName, Sizable};

/// A caption button, as Windows draws its own.
const BUTTON_WIDTH: f32 = 46.;
const BUTTON_HEIGHT: f32 = 32.;
/// Keep on top, minimize, maximize and close.
const BUTTONS: f32 = 4.;
/// The preference that keeps the main window above other windows.
pub(super) const KEEP_ON_TOP: &str = "alwaysOnTop";
/// The keep-on-top button, and its test anchor.
pub(super) const KEEP_ON_TOP_BUTTON: &str = "caption-on-top";

/// The width of the caption buttons at the end of the title row.
pub(super) fn width() -> Pixels {
    px(BUTTON_WIDTH * BUTTONS)
}

impl Desktop {
    /// Whether the main window stays above other windows.
    pub(super) fn keeps_on_top(&self) -> bool {
        self.prefs.get(KEEP_ON_TOP, false)
    }

    /// Keeps the main window above other windows, or returns it to the
    /// normal level. Test instances keep their place at the bottom of the
    /// desktop: above every window, one would cover the desktop in use.
    pub(super) fn keep_on_top(&mut self, on: bool) {
        self.prefs.set(KEEP_ON_TOP, on);
        if !self.options.test {
            crate::platform::keep_on_top(&self.host, on);
        }
    }

    /// The title row: `content` drags the window, and so does the room
    /// under the caption buttons.
    pub(super) fn title_row(
        &self,
        content: impl IntoElement,
        height: Pixels,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        div()
            .id("title-bar")
            .h(height)
            .flex_shrink_0()
            .flex()
            .flex_row()
            // A press here moves the window; it never starts a text
            // selection. A press on the caption arrives through the hit
            // test, and the move loop swallows its release, so a selection
            // started by it would follow the pointer afterwards.
            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                GlobalState::suppress_text_selection(cx)
            })
            .child(
                div()
                    .h_full()
                    .w_0()
                    .flex_grow(1.)
                    .min_w_0()
                    .window_control_area(WindowControlArea::Drag)
                    .child(content),
            )
            .child(
                div()
                    .h_full()
                    .flex_shrink_0()
                    .flex()
                    .flex_col()
                    .child(self.caption_buttons(window, cx))
                    .child(
                        div()
                            .w_full()
                            .flex_grow(1.)
                            .window_control_area(WindowControlArea::Drag),
                    ),
            )
    }

    /// Keep on top, minimize, maximize (restore while maximized) and close.
    /// Their colours are the component library's caption roles: the
    /// secondary fill, and Windows red under a white glyph for close.
    fn caption_buttons(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let ink = theme.foreground;
        let plain = (
            theme.secondary_hover,
            theme.secondary_active,
            theme.secondary_foreground,
        );
        let close = (theme.danger, theme.danger_active, theme.danger_foreground);
        let button = move |id: &'static str, (hover, press, hover_ink): (Hsla, Hsla, Hsla)| {
            div()
                .id(id)
                .relative()
                .flex_shrink_0()
                .w(px(BUTTON_WIDTH))
                .h(px(BUTTON_HEIGHT))
                .flex()
                .items_center()
                .justify_center()
                .text_color(ink)
                .hover(move |s| s.bg(hover).text_color(hover_ink))
                .active(move |s| s.bg(press).text_color(hover_ink))
        };
        let native = |id, area, glyph: IconName, colors| {
            button(id, colors)
                .window_control_area(area)
                .child(Icon::new(glyph).small())
        };
        let on_top = self.keeps_on_top();
        let control: SharedString = KEEP_ON_TOP_BUTTON.into();
        let title = label(if on_top {
            "shell.window.onTopOff"
        } else {
            "shell.window.onTop"
        });
        let p = self.palette;
        let keep = button(KEEP_ON_TOP_BUTTON, plain)
            .role(Role::Button)
            .aria_label(label("shell.window.onTop"))
            .aria_toggled(if on_top {
                Toggled::True
            } else {
                Toggled::False
            })
            .tooltip(move |window, cx| widgets::tooltip(title.clone(), window, cx))
            // Kept on top, the pin is filled with the accent.
            .child(self.moving_icon(
                &control,
                if on_top { "PinFilledIcon" } else { "PinIcon" },
                Some(if on_top { p.accent } else { ink }),
                Some(px(14.)),
            ))
            .child(self.icon_probe(&control))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _, window, cx| {
                this.act(
                    Action::Preference(KEEP_ON_TOP.into(), json!(!on_top)),
                    window,
                    cx,
                )
            }));
        div()
            .flex()
            .h(px(BUTTON_HEIGHT))
            .child(keep)
            .child(native(
                "minimize",
                WindowControlArea::Min,
                IconName::WindowMinimize,
                plain,
            ))
            .child(if window.is_maximized() {
                native(
                    "restore",
                    WindowControlArea::Max,
                    IconName::WindowRestore,
                    plain,
                )
            } else {
                native(
                    "maximize",
                    WindowControlArea::Max,
                    IconName::WindowMaximize,
                    plain,
                )
            })
            .child(native(
                "close",
                WindowControlArea::Close,
                IconName::WindowClose,
                close,
            ))
    }
}
