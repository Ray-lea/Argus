//! Application layout uses the native component rhythm and theme roles.
use super::*;
use gpui_kit::base::ElementExt;
use gpui_kit::component::ActiveTheme;

/// The side navigation's default expanded width is 184 px.
/// Dragging its edge adjusts it within these
/// limits; collapsed, it is a 48 px rail.
pub(in crate::frontend) const NAV_WIDTH: f32 = 184.;
const NAV_MIN_WIDTH: f32 = 150.;
const NAV_MAX_WIDTH: f32 = 320.;
const NAV_RAIL: f32 = 48.;
const NAV_INSET: f32 = 4.;
/// Navigation width change per arrow key on its edge.
const NAV_KEY_STEP: f32 = 16.;
/// The least of the page bar's drag stretch, before it steps down a level.
const BAR_STRETCH: f32 = 16.;
/// The page bar's inset after "More", as the title row's before the caption buttons.
const BAR_INSET: f32 = 12.;
/// The page bar's "More" button, and the status bar's refresh control: their
/// menus hang from them.
const MORE: &str = "toolbar-more";
const RATE: &str = "status-rate";
/// The search box's shortcut, as the menus write shortcuts.
const SEARCH_HINT: &str = "Ctrl+F";
/// The search box's share of the window's width, between its placeholder's
/// width and `searchWidth`.
const SEARCH_SHARE: f32 = 0.3;
/// The refresh rates on offer, in milliseconds.
const INTERVALS: [u64; 4] = [500, 1000, 2000, 5000];
/// Where a menu opens from its control.
#[derive(Clone, Copy)]
pub(in crate::frontend) enum Side {
    /// Under its left edge.
    Under,
    /// Under it, by its right edge.
    Below,
    /// Above it, by its right edge.
    Above,
}
impl Desktop {
    pub(in crate::frontend) fn set_theme(
        &mut self,
        dark: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        theme::apply(
            if dark {
                ThemeMode::Dark
            } else {
                ThemeMode::Light
            },
            window,
            cx,
        );
        scrollbar::style_library(dark, cx);
        self.palette = Palette::current(cx);
        for (table, frame) in self.tables.values() {
            table.update(cx, |_, cx| cx.notify());
            frame.update(cx, |_, cx| cx.notify());
        }
        self.restyle_monitors(cx);
        cx.notify();
    }
    /// A saved or dragged navigation width within its limits.
    pub(in crate::frontend) fn clamp_nav_width(width: f32) -> f32 {
        if width.is_finite() {
            width.round().clamp(NAV_MIN_WIDTH, NAV_MAX_WIDTH)
        } else {
            NAV_WIDTH
        }
    }
    /// The side navigation's width now: its expanded width, or the rail.
    pub(in crate::frontend) fn side_nav_width(&self) -> f32 {
        if self.nav_expanded {
            self.nav_width
        } else {
            NAV_RAIL
        }
    }
    /// The expanded side navigation's right edge: drag it, use the arrow keys,
    /// Home or End, or double-click to go back to the default width.
    fn nav_splitter(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = self.palette;
        let focus = window
            .use_keyed_state("nav-splitter-focus", cx, |_, cx| {
                cx.focus_handle().tab_stop(true)
            })
            .read(cx)
            .clone();
        let dragging = self.drag.as_ref().is_some_and(|(kind, _, _)| kind == "nav");
        let shown = dragging || focus.is_focused(window);
        let grip = div()
            .absolute()
            .top(relative(0.5))
            .mt(px(-16.))
            .left(px(3.))
            .w(px(2.))
            .h(px(32.))
            .rounded(px(1.))
            .bg(if dragging {
                p.accent
            } else {
                theme::token("stroke-strong", p.dark)
            })
            .when(!shown, |grip| {
                grip.opacity(0.)
                    .group_hover("nav-splitter", |style| style.opacity(1.))
            });
        div()
            .id("nav-splitter")
            .group("nav-splitter")
            .absolute()
            .top_0()
            .bottom_0()
            .right(px(-4.))
            .w(px(8.))
            .cursor_col_resize()
            .track_focus(&focus)
            .role(Role::Splitter)
            .aria_label(label("shell.nav.resize"))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    if event.click_count >= 2 {
                        this.drag = None;
                        this.nav_width = NAV_WIDTH;
                        this.prefs.set("navWidth", this.nav_width);
                    } else {
                        this.drag = Some(("nav".into(), event.position, this.nav_width));
                    }
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                let width = match event.keystroke.key.as_str() {
                    "left" => this.nav_width - NAV_KEY_STEP,
                    "right" => this.nav_width + NAV_KEY_STEP,
                    "home" => NAV_MIN_WIDTH,
                    "end" => NAV_MAX_WIDTH,
                    _ => return,
                };
                this.nav_width = Self::clamp_nav_width(width);
                this.prefs.set("navWidth", this.nav_width);
                cx.stop_propagation();
                cx.notify();
            }))
            .child(grip)
            .into_any_element()
    }
    pub(in crate::frontend) fn navigation(
        &self,
        top: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.palette;
        let selected = PAGES
            .iter()
            .position(|page| page.0 == self.page)
            .unwrap_or(0);
        let names = PAGES.map(|(page, _)| label(&format!("shell.page.{page}")));
        // On a user-narrowed rail, give the translated labels room before
        // spending the remaining width on the two insets and the icon gap.
        let spacing = if !top && self.nav_expanded {
            let widest = names
                .iter()
                .map(|name| theme::text_width(name, theme::font(13.), window, cx))
                .max()
                .unwrap_or_default();
            let space =
                self.nav_width - 2. * NAV_INSET - f32::from(window.rem_size() + widest.ceil());
            (space / 3.).floor().clamp(0., 12.)
        } else {
            12.
        };
        let regions = *self.navigation_regions.borrow();
        let owner = cx.entity().downgrade();
        let hover = p.hover;
        let press = p.press;
        let hover_strong = p.hover_strong;
        let update_waiting = self
            .data
            .get("updates")
            .is_some_and(|s| matches!(text(s, "state"), "ready" | "available"));
        // The names fade in as the pane opens (`fade-in`, 167 ms linear).
        let fade_names = !cx.reduce_motion();
        let marker_width = theme::dimension("navigationIndicatorWidth");
        // Where the selection mark is: across on top, down at the side. The
        // regions describe the position they were measured in (the entries'
        // container is a row on top, a column at the side); just after a
        // switch they still describe the other one, and the mark waits the
        // frame it takes to measure the new one.
        let measured_top = regions[0].size.width > regions[0].size.height;
        let marker =
            (measured_top == top && regions[selected + 1].size.width > px(0.)).then(|| {
                let mut mark = self.ui.nav_mark.get();
                if mark.top != top || (mark.selected == selected && mark.size != regions[0].size) {
                    mark.epoch += 1;
                }
                mark.top = top;
                mark.selected = selected;
                mark.size = regions[0].size;
                self.ui.nav_mark.set(mark);
                let chosen = regions[selected + 1];
                let (id, target) = if top {
                    (
                        "page-navigation-indicator",
                        chosen.center().x - regions[0].left() - marker_width / 2.,
                    )
                } else {
                    (
                        "page-navigation-pane-indicator",
                        chosen.center().y - regions[0].top() - px(8.),
                    )
                };
                gpui_kit::base::motion::transition(
                    format!("{id}-{}", mark.epoch),
                    target,
                    theme::transition_slow(cx),
                    window,
                    cx,
                )
            });
        let entry = |index: usize, page: &'static str, glyph: &'static str| {
            let name = &names[index];
            let chosen = index == selected;
            let control = SharedString::from(format!("page-nav-{page}"));
            let title = tr!("shell.pageTitle", page = name.clone(), key = index + 1);
            let has_dot = page == "settings" && update_waiting;
            let dot =
                move || has_dot.then(|| div().absolute().size(px(6.)).rounded_full().bg(p.accent));
            gpui_kit::base::Tab::new(("page", index))
                .selected(chosen)
                .line_height(theme::line_height())
                .set_position(index + 1, PAGES.len())
                .accessibility_label(name.clone())
                .relative()
                .flex()
                .flex_shrink_0()
                .items_center()
                .rounded(px(4.))
                .px(px(spacing))
                .text_color(if chosen { p.ink } else { p.muted })
                // The chosen page keeps its fill while hovered or pressed
                // (`.nav-item[aria-selected]` follows `:hover` and `:active`).
                .hover(move |s| {
                    s.bg(if chosen { hover_strong } else { hover })
                        .text_color(p.ink)
                })
                .active(move |s| s.bg(if chosen { hover_strong } else { press }))
                .tooltip(move |window, cx| widgets::tooltip(title.clone(), window, cx))
                .on_prepaint(track_region(
                    index + 1,
                    self.navigation_regions.clone(),
                    owner.clone(),
                ))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.act(Action::Page(page.into()), window, cx);
                }))
                .when(page == "history", |tab| {
                    tab.on_hover(cx.listener(|this, hovered: &bool, window, cx| {
                        if *hovered {
                            this.history_prefetch(window, cx);
                        }
                    }))
                })
                .when(top, |tab| {
                    // Reserve the selected weight so choosing a tab never moves its neighbors.
                    tab.h(px(32.)).child(
                        div()
                            .relative()
                            .child(
                                div()
                                    .font_weight(theme::LABEL_WEIGHT)
                                    .invisible()
                                    .child(name.clone()),
                            )
                            .child(
                                div()
                                    .absolute()
                                    .inset_0()
                                    .flex()
                                    .justify_center()
                                    .when(chosen, |d| d.font_weight(theme::LABEL_WEIGHT))
                                    .child(name.clone()),
                            )
                            .children(dot().map(|dot| dot.top(px(-1.)).right(px(-7.)))),
                    )
                })
                .when(!top, |tab| {
                    tab.h(px(36.))
                        .justify_start()
                        .gap(px(spacing))
                        .when(chosen, |tab| tab.bg(hover_strong))
                        .child(
                            div()
                                .relative()
                                .flex_shrink_0()
                                // In the entry's colour, which brightens on hover.
                                .child(self.moving_icon(&control, glyph, None, None))
                                .children(dot().map(|dot| dot.top(px(-2.)).right(px(-3.)))),
                        )
                        .child(self.icon_probe(&control))
                        .when(self.nav_expanded, |tab| {
                            let name = div().min_w_0().truncate().child(name.clone());
                            tab.child(if fade_names {
                                name.with_animation(
                                    ("page-navigation-name", index),
                                    Animation::new(Duration::from_millis(167)),
                                    |name, delta| name.opacity(delta),
                                )
                                .into_any_element()
                            } else {
                                name.into_any_element()
                            })
                        })
                })
        };
        if top {
            // Up to the last compact level the tabs keep one row and the page
            // bar steps down a level instead; then they wrap beside "More".
            let wrap = self.ui.toolbar_fit.borrow().level >= 2;
            let tabs = gpui_kit::base::Tabs::new("page-navigation")
                .relative()
                .flex()
                .flex_wrap()
                .gap(px(2.))
                .when(wrap, |tabs| tabs.flex_shrink(1.).min_w_0())
                .when(!wrap, |tabs| tabs.flex_shrink_0())
                .on_prepaint(track_region(
                    0,
                    self.navigation_regions.clone(),
                    owner.clone(),
                ))
                .children(
                    PAGES
                        .iter()
                        .enumerate()
                        .map(|(index, (page, glyph))| entry(index, page, glyph)),
                )
                .when_some(marker, |tabs, marker_x| {
                    tabs.child(
                        div()
                            .absolute()
                            // 2 px above the bottom of the chosen tab's row.
                            .top(
                                regions[selected + 1].bottom()
                                    - regions[0].top()
                                    - px(2.)
                                    - theme::dimension("navigationIndicatorHeight"),
                            )
                            .left(marker_x)
                            .w(marker_width)
                            .h(theme::dimension("navigationIndicatorHeight"))
                            .rounded(px(2.))
                            .bg(p.accent),
                    )
                });
            return self.page_bar(tabs.into_any_element(), window, cx);
        }
        let toggle_tip = label(if self.nav_expanded {
            "shell.nav.collapse"
        } else {
            "shell.nav.expand"
        });
        let mut items = gpui_kit::base::Tabs::new("page-navigation")
            .relative()
            .flex()
            .flex_col()
            .flex_1()
            .gap(px(4.))
            .on_prepaint(track_region(
                0,
                self.navigation_regions.clone(),
                owner.clone(),
            ));
        for (index, (page, glyph)) in PAGES.iter().enumerate() {
            // Components, Settings and About sit apart at the end.
            if *page == "components" {
                items = items.child(div().flex_1());
            }
            items = items.child(entry(index, page, glyph));
        }
        if let Some(marker_y) = marker {
            items = items.child(
                div()
                    .absolute()
                    .left_0()
                    .top(marker_y)
                    .w(px(3.))
                    .h(px(16.))
                    .rounded(px(2.))
                    .bg(p.accent),
            );
        }
        div()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .gap(px(4.))
            .px(px(NAV_INSET))
            .pb(px(8.))
            .relative()
            .w(px(self.side_nav_width()))
            .child(
                // An entry like the pages': its icon lines up with theirs,
                // at the start of the open pane (Button centres by default).
                Button::new("nav-toggle")
                    .h(px(36.))
                    .px(px(12.))
                    .flex()
                    .items_center()
                    .justify_start()
                    .rounded(px(4.))
                    .text_color(p.muted)
                    .hover(move |s| s.bg(hover).text_color(p.ink))
                    .active(move |s| s.bg(press))
                    .accessibility_label(toggle_tip)
                    .aria_expanded(self.nav_expanded)
                    .child(theme::glyph("MenuIcon"))
                    .tooltip({
                        // Named for what a click does now: a tip that comes
                        // up after a click must not name the previous state.
                        let owner = owner.clone();
                        move |window, cx| {
                            let expanded = owner
                                .upgrade()
                                .is_some_and(|desktop| desktop.read(cx).nav_expanded);
                            widgets::tooltip(
                                label(if expanded {
                                    "shell.nav.collapse"
                                } else {
                                    "shell.nav.expand"
                                }),
                                window,
                                cx,
                            )
                        }
                    })
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.act(
                            Action::Preference("navExpanded".into(), json!(!this.nav_expanded)),
                            window,
                            cx,
                        )
                    })),
            )
            .child(items)
            .when(self.nav_expanded, |d| {
                d.child(self.nav_splitter(window, cx))
            })
            .into_any_element()
    }
    /// A toolbar button with the shared button style: 32 px, a stroke border with a
    /// stronger bottom edge, the control surface, and the accent while pressed.
    /// Without text it is a square icon button.
    pub(in crate::frontend) fn tool_button(
        &self,
        id: impl Into<SharedString>,
        text: impl Into<SharedString>,
        glyph: Option<&str>,
        action: Action,
        pressed: bool,
        cx: &Context<Self>,
    ) -> Button {
        self.tool_face(id, text, glyph, pressed)
            .on_click(cx.listener(move |this, _, window, cx| this.act(action.clone(), window, cx)))
    }
    /// A toolbar button's face without what a click does.
    pub(in crate::frontend) fn tool_face(
        &self,
        id: impl Into<SharedString>,
        text: impl Into<SharedString>,
        glyph: Option<&str>,
        pressed: bool,
    ) -> Button {
        let text = text.into();
        let id: SharedString = id.into();
        let p = self.palette;
        let color = if pressed { p.accent } else { p.ink };
        Button::new(id.clone())
            .relative()
            .h(px(32.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .gap(px(6.))
            .when(text.is_empty(), |b| b.w(px(32.)))
            .when(!text.is_empty(), |b| b.px(px(12.)))
            .rounded(px(4.))
            .border_1()
            .border_color(p.stroke)
            .bg(p.control_face)
            .text_color(color)
            .hover(move |s| s.bg(p.control_hover))
            .active(move |s| s.bg(p.press).text_color(p.muted))
            .child(widgets::control_edge(&p))
            .accessibility_label(text.clone())
            .when_some(glyph, |b, name| {
                b.child(self.moving_icon(&id, name, Some(color), None))
                    .child(self.icon_probe(&id))
            })
            .when(!text.is_empty(), |b| b.child(text))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
    }
    /// The title row: the brand, the current page's search box and the
    /// window's caption buttons. Everything else in it drags the window.
    pub(in crate::frontend) fn toolbar(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.palette;
        let owner = cx.entity().downgrade();
        let height = theme::dimension("toolbarHeight");
        let state = self.scrim.clone();
        let caption_scrim = self
            .caption_scrim
            .get_or_insert_with(|| {
                cx.new(|_| dialogs::Scrim {
                    state,
                    captions: true,
                })
            })
            .clone();
        let start = div()
            .flex()
            .items_center()
            .gap(px(10.))
            .pl(px(4.))
            .flex_shrink_0()
            // Spanning the side pane lines the search box up with the page.
            .when(!self.nav_top, |d| {
                d.min_w(px(self.side_nav_width()) - px(8.))
            })
            .text_size(theme::font(14.))
            .text_color(p.ink)
            // 24 px: a pixel master at every standard scaling
            // (24/30/36/42/48 device pixels, as on the taskbar).
            .child(widgets::logo(24.))
            .child(argus_app::config::app_title())
            .on_prepaint(track_title(0, self.toolbar_regions.clone(), owner.clone()));
        let mut toolbar = div()
            .relative()
            .w_full()
            .h(height)
            .flex_shrink_0()
            .flex()
            .flex_col()
            .bg(p.background)
            // Over the row's background and under the caption buttons: the
            // part of the dialogs' dim that their layer leaves out.
            .child(
                caption_scrim.cached(
                    StyleRefinement::default()
                        .absolute()
                        .top_0()
                        .right_0()
                        .w(caption::width())
                        .h(height),
                ),
            )
            .child(
                self.title_row(
                    div()
                        // Exactly the room before the caption buttons.
                        .size_full()
                        .overflow_hidden()
                        .flex()
                        .items_center()
                        .pl(px(12.))
                        .pr(px(4.))
                        .child(start)
                        .on_prepaint(track_title(2, self.toolbar_regions.clone(), owner)),
                    height,
                    window,
                    cx,
                ),
            );
        if self.has_search() {
            let regions = *self.toolbar_regions.borrow();
            let left = regions[0].right() + px(8.);
            // The row's 4 px inset before the caption buttons, and 8 px more.
            let right = regions[2].right() - px(12.);
            if right > left {
                let filled = !self.query.trim().is_empty();
                let focused = self.search.read(cx).focus_handle(cx).is_focused(window);
                let empty = self.search.read(cx).value().is_empty();
                let placeholder: SharedString = label(self.search_placeholder()).into();
                // A share of the window's width: narrow windows keep more of
                // the title row to drag by. No narrower than its placeholder,
                // no wider than `searchWidth`, and never past its neighbours.
                let fit = self.search_fit(window, cx);
                let width = (window.viewport_size().width * SEARCH_SHARE)
                    .max(fit)
                    .min(theme::dimension("searchWidth"))
                    .min(right - left);
                // The shortcut rests at the end of an empty, idle box that
                // has room for it beside the whole placeholder.
                let hint_width = theme::text_width(SEARCH_HINT, theme::font(11.), window, cx)
                    .ceil()
                    + px(10. + 2. + 8.);
                let hint = empty && !focused && width >= fit + hint_width;
                // A Fluent text box (`.search input`): the control face with a
                // stronger bottom edge; focused, a 2 px accent bottom on the raised
                // layer; while it filters the list, it wears the accent.
                let hover = p.control_hover;
                let (border, background) = if filled {
                    (
                        p.accent.opacity(0.55),
                        // `color-mix(in srgb, var(--accent) 9%, var(--control))`
                        theme::mix(p.control_face, p.accent, 0.09),
                    )
                } else if focused {
                    (p.stroke, p.layer_strong)
                } else {
                    (p.stroke, p.control_face)
                };
                // The accent bottom: 2 px while focused, 1 px while filtering.
                let edge = if focused {
                    Some(px(2.))
                } else {
                    filled.then_some(px(1.))
                };
                let face = div()
                    .relative()
                    .size_full()
                    .flex()
                    .items_center()
                    .rounded(px(4.))
                    .border_1()
                    .border_color(border)
                    .bg(background)
                    .when(!filled && !focused, |d| d.hover(move |s| s.bg(hover)))
                    .when(edge.is_none(), |d| d.child(widgets::control_edge(&p)))
                    // The icon 10 px from the edge and the text 34 px inside it
                    // (`.search > svg`, `padding-left: 34px`).
                    .when(empty, |d| {
                        d.child(widgets::placeholder(
                            placeholder.clone(),
                            px(34.),
                            if hint { hint_width } else { px(12.) },
                            &p,
                        ))
                    })
                    .child(
                        widgets::input(&self.search)
                            .appearance(false)
                            .cleanable(true)
                            .h_full()
                            .pl(px(9.))
                            .gap(px(9.))
                            .text_size(theme::font(13.))
                            .aria_label(placeholder)
                            .prefix(icon("SearchIcon", if filled { p.accent } else { p.muted })),
                    )
                    .when(hint, |d| {
                        d.child(
                            div()
                                .absolute()
                                .top_0()
                                .bottom_0()
                                .right(px(7.))
                                .flex()
                                .items_center()
                                .child(
                                    div()
                                        .px(px(5.))
                                        .border_1()
                                        .border_color(p.stroke_strong)
                                        .rounded(px(3.))
                                        .text_size(theme::font(11.))
                                        .text_color(p.faint)
                                        .child(SEARCH_HINT),
                                ),
                        )
                    });
                let anchors = self.toolbar_regions.clone();
                let owner = cx.entity().downgrade();
                let track = track_title(1, anchors, owner);
                let centered = (window.viewport_size().width - width) / 2.;
                let search = div()
                    .id("toolbar-search")
                    .relative()
                    .h(px(32.))
                    .occlude()
                    .child(face)
                    .when_some(edge, |d, width| {
                        d.child(widgets::accent_edge(p.accent, width))
                    })
                    .child(
                        canvas(
                            move |bounds, window, cx| track(bounds, window, cx),
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .inset_0(),
                    )
                    .absolute()
                    .left(centered.max(left).min(right - width))
                    .top((height - px(32.)) / 2.)
                    .w(width);
                toolbar = toolbar.child(search);
            }
        }
        toolbar.into_any_element()
    }
    /// The page bar: `leading` (the tabs, or the page's name), a stretch that
    /// drags the window, the page's own controls and "More". At the last
    /// compact level the controls take a row of their own below.
    fn page_bar(
        &self,
        leading: AnyElement,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.palette;
        // The compact level `fit_toolbar` chose for this frame; the regions
        // measured below belong to it.
        let level = {
            let mut fit = self.ui.toolbar_fit.borrow_mut();
            fit.drawn = Some(fit.level);
            fit.level
        };
        let owner = cx.entity().downgrade();
        let regions = self.bar_regions.clone();
        let settling = self.bar_settling.clone();
        let probe = move |index: usize| {
            let track = track_bar(
                index,
                regions.clone(),
                owner.clone(),
                settling.clone(),
                level,
            );
            canvas(
                move |bounds, window, cx| track(bounds, window, cx),
                |_, _, _, _| {},
            )
            .absolute()
            .inset_0()
        };
        let controls = self.page_bar_controls(level, window, cx);
        let (inline, own_row) = if level < 2 {
            (controls, None)
        } else {
            (None, controls)
        };
        let trailing = div()
            .relative()
            .flex()
            .items_center()
            .gap_2()
            .flex_shrink_0()
            .occlude()
            .when_some(inline, |d, controls| {
                d.child(controls)
                    .child(div().w(px(1.)).h(px(20.)).mx_1().bg(p.stroke))
            })
            .child(self.more_button(cx))
            .child(probe(0));
        let stretch = div()
            .relative()
            .flex_1()
            .min_w(px(BAR_STRETCH))
            .h(px(32.))
            .window_control_area(WindowControlArea::Drag)
            .child(probe(2));
        div()
            .relative()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .pl(px(if self.nav_top { 12. } else { 16. }))
            .pr(px(BAR_INSET))
            .pb(px(6.))
            .child(
                div()
                    .flex()
                    .items_start()
                    .child(leading)
                    .child(stretch)
                    .child(trailing),
            )
            .when_some(own_row, |d, controls| {
                d.child(div().flex().pt(px(6.)).child(controls))
            })
            .child(probe(1))
            .into_any_element()
    }
    /// The current page's own controls in the page bar, if it has any. At
    /// compact level 1 the labelled choices drop down from a button instead.
    fn page_bar_controls(
        &self,
        level: u8,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.has_controls() {
            return None;
        }
        let compact = level == 1;
        let row = div().flex().items_center().gap_2().flex_shrink_0();
        // The services and connections filters are in their columns' headers
        // (`NativeTable::header_filter`).
        let row = match self.page.as_str() {
            "processes" => {
                // Icons only: the names are in the tooltips.
                let views = [
                    ("apps", "AppsIcon"),
                    ("tree", "TreeIcon"),
                    ("list", "ListIcon"),
                ]
                .into_iter()
                .map(|(mode, glyph)| {
                    (
                        String::new(),
                        Some(glyph),
                        Action::View(mode.into()),
                        self.view == mode,
                    )
                })
                .collect();
                row.child(self.segments("process-view", views, cx))
                    .child(self.visibility_controls(cx))
                    .child(self.details_button(cx))
            }
            "services" => row.child(self.service_actions(cx)),
            "network" => row.child(self.visibility_controls(cx)),
            _ => row.child(self.page_controls(compact, window, cx)),
        };
        Some(row.into_any_element())
    }
    /// Beside the side navigation: the page's name leads the page bar.
    pub(in crate::frontend) fn page_header(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.palette;
        let title = div()
            .flex()
            .items_center()
            .h(px(32.))
            .flex_shrink_0()
            .font_family(theme::display_font(cx))
            .text_size(theme::font(20.))
            .font_weight(theme::LABEL_WEIGHT)
            .text_color(p.ink)
            .whitespace_nowrap()
            .child(label(&format!("shell.page.{}", self.page)));
        self.page_bar(title.into_any_element(), window, cx)
    }
    /// The details pane's button: three modes in turn (always, when a process
    /// is selected, never), a right click picks one. Lit while the pane can
    /// show; the automatic mode has its own glyph.
    fn details_button(&self, cx: &mut Context<Self>) -> AnyElement {
        let mode = self.details_mode;
        let name = |mode: DetailsMode| label(&format!("shell.details.mode.{}", mode.key()));
        let tip = format!(
            "{}\n{}",
            tr!("shell.details.current", mode = name(mode)),
            tr!("shell.details.next", mode = name(mode.next()))
        );
        self.tool_button(
            "details",
            String::new(),
            Some(if mode == DetailsMode::Auto {
                "PanelAutoIcon"
            } else {
                "PanelIcon"
            }),
            Action::Details(None),
            mode != DetailsMode::Closed,
            cx,
        )
        .accessibility_label(label("shell.details.label"))
        .tooltip(move |window, cx| widgets::tooltip(tip.clone(), window, cx))
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(|this, _, window, cx| {
                cx.stop_propagation();
                this.details_menu(window, cx);
            }),
        )
        .into_any_element()
    }
    /// "More", at the page bar's end on every page: the tools that belong to
    /// no page.
    fn more_button(&self, cx: &mut Context<Self>) -> AnyElement {
        let title = label("shell.more.label");
        div()
            .relative()
            .flex_shrink_0()
            .child(
                self.tool_face(MORE, "", Some("MoreIcon"), false)
                    .accessibility_label(title.clone())
                    .tooltip(move |window, cx| widgets::tooltip(title.clone(), window, cx))
                    .on_click(cx.listener(|this, _, window, cx| this.open_more_menu(window, cx))),
            )
            .child(anchor_probe(MORE, self.ui.anchors.clone()))
            .into_any_element()
    }
    fn open_more_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut items = vec![
            MenuItem::new(
                label("shell.more.runTask"),
                Action::Dialog("task".into(), json!({})),
            )
            .icon("RunTaskIcon")
            .hint("Ctrl+N"),
            MenuItem::new(
                label("shell.more.fileLock"),
                Action::Dialog("filelock".into(), json!({})),
            )
            .icon("LockIcon")
            .detail(label("shell.more.fileLockDetail")),
        ];
        if self.page == "history" {
            items.push(MenuItem::separator());
            items.extend(self.history_more_items());
        }
        if self.page == "services" {
            items.push(
                MenuItem::new(
                    label("services.control.openConsoleTitle"),
                    Action::Invoke("open_services_console".into(), json!({})),
                )
                .icon("ExternalIcon"),
            );
        }
        // On the history page the chart's keys and gestures come open.
        let open = if self.page == "history" {
            json!({ "open": { "history": true, "historyMouse": true } })
        } else {
            json!({})
        };
        items.push(MenuItem::separator());
        items.push(
            MenuItem::new(
                label("settings.shortcuts.title"),
                Action::Dialog("shortcuts".into(), open),
            )
            .icon("KeyboardIcon"),
        );
        self.open_anchored(items, MORE, Side::Below, window, cx);
    }
    /// A list column's filter, under its button in the column's header.
    pub(in crate::frontend) fn open_column_filter(
        &mut self,
        column: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(filter) = tables::column_filter(&self.page, column) else {
            return;
        };
        let items = self.filter_menu(filter);
        self.open_anchored(items, &filter.anchor(), Side::Under, window, cx);
    }
    /// Opens `items` from the control recorded as `anchor`: under its left
    /// edge, or by its right edge (above or below it), turning toward the
    /// free side as menus do. Unrecorded, the menu opens at the pointer.
    pub(in crate::frontend) fn open_anchored(
        &mut self,
        items: Vec<MenuItem>,
        anchor: &str,
        side: Side,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A second click on the control closes its menu, as in Windows.
        if self.menu_just_shut(anchor) {
            return;
        }
        let bounds = self.ui.anchors.borrow().get(anchor).copied();
        match bounds {
            Some(b) => {
                let at = match side {
                    Side::Under => point(b.left(), b.bottom() + px(4.)),
                    Side::Below => point(b.right(), b.bottom() + px(4.)),
                    Side::Above => point(b.right(), b.top() - px(4.)),
                };
                self.open_popup_at(items, at, window, cx);
            }
            None => self.open_popup(items, window, cx),
        }
        self.anchor_menu(anchor);
    }
    pub(in crate::frontend) fn has_search(&self) -> bool {
        matches!(
            self.page.as_str(),
            "processes" | "services" | "startup" | "network"
        )
    }
    /// Pages with controls of their own in the page bar (`.page-controls`).
    fn has_controls(&self) -> bool {
        matches!(
            self.page.as_str(),
            "processes" | "services" | "network" | "history"
        )
    }
    /// The update state the toolbar's update button shows, if it shows one.
    fn update_button(&self) -> Option<&Value> {
        self.data
            .get("updates")
            .filter(|s| matches!(text(s, "state"), "ready" | "available" | "downloading"))
            .filter(|s| {
                self.prefs.get::<String>("updateDismissed", String::new()) != text(s, "version")
            })
    }
    /// The narrowest the search box gets: its placeholder and the room around
    /// it (`--fit` in the interface's SearchBox), measured once per text and size.
    fn search_fit(&self, window: &Window, cx: &App) -> Pixels {
        let placeholder = label(self.search_placeholder());
        let size = theme::font(13.);
        if let Some((text, at, fit)) = self.ui.search_fit.borrow().as_ref()
            && *text == placeholder
            && *at == size
        {
            return *fit;
        }
        // The icon and padding on the left, padding on the right, the borders.
        let fit = theme::text_width(&placeholder, size, window, cx).ceil() + px(34. + 12. + 2.);
        *self.ui.search_fit.borrow_mut() = Some((placeholder, size, fit));
        fit
    }
    /// Keeps the page bar inside the window, as the interface's `fitToolbar`:
    /// while its end controls run past the row (the drag stretch before them
    /// already at its least) the bar goes one compact level further each
    /// frame, learning the room the level wanted; it goes back once the bar
    /// has that room. Level 1 turns the labelled choices into drop-downs
    /// (pages without any skip it), level 2 moves the page's controls to a
    /// row of their own. Each page, text size, language, navigation and the
    /// history page's "Latest" button keeps what it learned: coming back, the
    /// bar starts at the level that room allows. Until a level fits, the
    /// frames drawn at the others are not presented (`track_bar`). Returns the
    /// level to draw.
    pub(in crate::frontend) fn fit_toolbar(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> u8 {
        let key = format!(
            "{}|{}|{}|{}|{}",
            self.page,
            self.font_size,
            self.nav_top,
            argus_core::i18n::locale() as u8,
            // The history page's live button names what a click does.
            self.page == "history" && self.history_follows()
        );
        let regions = *self.bar_regions.borrow();
        // The bar's room: the window, less the side navigation beside it.
        let room = window.viewport_size().width
            - if self.nav_top {
                px(0.)
            } else {
                px(self.side_nav_width())
            };
        let mut state = self.ui.toolbar_fit.borrow_mut();
        if state.key != key {
            let mut learned = std::mem::take(&mut state.learned);
            let previous = std::mem::take(&mut state.key);
            if !previous.is_empty() {
                learned.insert(previous, state.need);
            }
            let need = learned.get(&key).copied().unwrap_or_default();
            // The first level this room does not rule out (an unknown need fits).
            let mut level = 0;
            while level < 2
                && need[level as usize] > px(0.)
                && room + px(0.5) < need[level as usize]
            {
                level += 1;
            }
            *state = widgets::ToolbarFit {
                key,
                level,
                need,
                drawn: None,
                learned,
            };
            self.bar_settling.set(true);
        }
        let before = state.level;
        while state.level > 0 && room + px(0.5) >= state.need[state.level as usize - 1] {
            state.level -= 1;
        }
        // The regions measure the level last drawn: once the current one is
        // drawn, see whether it fits.
        if state.drawn == Some(state.level)
            && regions[0].size.width > px(0.)
            && regions[1].size.width > px(0.)
        {
            let overflow = regions[0].right() - (regions[1].right() - px(BAR_INSET));
            if overflow > px(0.5) && state.level < 2 {
                let level = state.level as usize;
                state.need[level] = room + overflow;
                if level == 0 && !self.has_choices() {
                    // Nothing to fold: level 1 would draw the same bar.
                    state.need[1] = room + overflow;
                    state.level = 2;
                } else {
                    state.level += 1;
                }
            } else {
                self.bar_settling.set(false);
            }
        }
        let level = state.level;
        if level != before {
            // Measure the new level on the next frame even if nothing in the
            // bar changes size.
            let owner = cx.entity().downgrade();
            window.defer(cx, move |_, cx| {
                let _ = owner.update(cx, |_, cx| cx.notify());
            });
        }
        level
    }
    /// Pages whose bar has labelled choices to fold into drop-downs (level 1).
    fn has_choices(&self) -> bool {
        false
    }
    /// The page bar's height below its first row: the controls' own row at
    /// the last compact level.
    fn bar_extra(&self, level: u8) -> Pixels {
        if level >= 2 && self.has_controls() {
            px(32. + 6.)
        } else {
            px(0.)
        }
    }
    /// What can be done to the selected service, after the services filter.
    fn service_actions(&self, cx: &Context<Self>) -> Div {
        let services = self.data.get("services").map(values).unwrap_or_default();
        let selected = self
            .selected
            .get("services")
            .and_then(|name| services.iter().find(|s| text(s, "name") == name));
        let pending = selected.is_none_or(|s| text(s, "state").ends_with("Pending"));
        let mut row = div().flex().items_center().gap_2();
        for (id, glyph, action, title, disabled) in [
            (
                "svc-start",
                "PlayIcon",
                "start",
                "common.start",
                pending || selected.is_some_and(|s| text(s, "state") == "running"),
            ),
            (
                "svc-stop",
                "StopIcon",
                "stop",
                "common.stop",
                pending || !selected.is_some_and(|s| boolean(s, "canStop")),
            ),
            (
                "svc-restart",
                "RestartIcon",
                "restart",
                "common.restart",
                pending || !selected.is_some_and(|s| boolean(s, "canStop")),
            ),
        ] {
            let name = selected
                .map(|s| text(s, "name").to_owned())
                .unwrap_or_default();
            let display = selected
                .map(|s| text(s, "displayName").to_owned())
                .unwrap_or_default();
            let tip = label(title);
            row = row.child(
                self.tool_button(
                    id,
                    String::new(),
                    Some(glyph),
                    Action::Invoke(
                        "control_service".into(),
                        json!({"name": name, "action": action, "display": display}),
                    ),
                    false,
                    cx,
                )
                .disabled(disabled)
                .when(disabled, |b| b.opacity(0.4))
                .accessibility_label(label(&format!("services.control.{action}")))
                .tooltip(move |window, cx| widgets::tooltip(tip.clone(), window, cx)),
            );
        }
        row
    }
    pub(in crate::frontend) fn statusbar(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = self.palette;
        let sys = &self.snapshot["system"];
        let version = match self.update_button() {
            Some(updates) => self.update_badge(updates, cx),
            None => self.version_badge(cx),
        };
        let mut figures = div()
            .flex()
            .flex_wrap()
            .flex_1()
            .min_w_0()
            // One line (`height: 1lh`): figures that do not fit drop out whole.
            .h(px(17.) + theme::text_growth(12., 1.4))
            .overflow_hidden()
            .gap_x(px(20.));
        let cached = crate::regional::number!("{:.1} GB", number(sys, "cached") / 1073741824.);
        for (key, value) in [
            (
                "shell.status.threads",
                theme::count(number(sys, "threadCount")),
            ),
            (
                "shell.status.handles",
                theme::count(number(sys, "handleCount")),
            ),
            ("shell.status.cached", cached),
            (
                "shell.status.pagedPool",
                theme::memory(number(sys, "kernelPaged")),
            ),
            (
                "shell.status.uptime",
                status_uptime(number(sys, "uptimeMs") / 1000.),
            ),
        ] {
            figures = figures.child(
                div()
                    .flex()
                    .gap_1()
                    .whitespace_nowrap()
                    .child(label(key))
                    .child(div().text_color(p.muted).child(value)),
            );
        }
        let privilege = if boolean(&self.data["info"], "elevated") {
            "admin"
        } else if self.privileged() {
            "service"
        } else {
            "standard"
        };
        let badge_color = match privilege {
            "admin" => p.accent,
            "service" => p.network,
            _ => p.muted,
        };
        let badge_border = if privilege == "standard" {
            p.stroke
        } else {
            badge_color.opacity(0.4)
        };
        let hint = label(&format!("shell.elevation.{privilege}Hint"));
        div()
            .pt(px(6.))
            .pb(px(8.))
            .px(px(16.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(20.))
            .text_size(theme::font(12.))
            .text_color(p.faint)
            .whitespace_nowrap()
            .child(version)
            .child(figures)
            .when(
                self.held_order
                    && self.pointer_in_table
                    && self.hover_holds_order()
                    && !self.paused,
                |d| {
                    // A badge in the accent, as the privilege badge beside it;
                    // the pointer, not the pause button beside it.
                    d.child(
                        div()
                            .id("order-held")
                            .flex()
                            .items_center()
                            .gap(px(5.))
                            .h(px(20.))
                            .my(px(-2.))
                            .px(px(8.))
                            .border_1()
                            .border_color(p.accent.opacity(0.4))
                            .bg(p.accent.opacity(0.08))
                            .rounded_full()
                            .text_color(p.accent)
                            .child(icon("PointerIcon", p.accent).size(px(12.)))
                            .child(label("shell.status.orderHeld"))
                            .tooltip(|window, cx| {
                                widgets::wrapping_tooltip(
                                    label("shell.status.orderHeldHint").into(),
                                    px(320.),
                                    window,
                                    cx,
                                )
                            }),
                    )
                },
            )
            .child(self.refresh_control(cx))
            .child(
                div()
                    .id("privilege-badge")
                    .flex()
                    .items_center()
                    .gap(px(5.))
                    .h(px(20.))
                    .my(px(-2.))
                    .px(px(8.))
                    .border_1()
                    .border_color(badge_border)
                    .rounded_full()
                    .text_color(badge_color)
                    .child(icon("ShieldIcon", badge_color).size(px(12.)))
                    .child(label(&format!("shell.elevation.{privilege}")))
                    .tooltip(move |window, cx| widgets::tooltip(hint.clone(), window, cx)),
            )
            .into_any_element()
    }
    /// The version, a test build's commit count with it (the hash is in the
    /// tooltip): it opens this version's notes, and copies the full version
    /// on a right click.
    fn version_badge(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = self.palette;
        let build = self.shown_version().to_owned();
        let copied = build.clone();
        let short = match build.split_once('+') {
            Some((release, meta)) => {
                format!("{release}+{}", meta.split('.').next().unwrap_or(meta))
            }
            None => build.clone(),
        };
        let version_hint = tr!(
            "shell.status.versionHint",
            version = build,
            build = self.build_label()
        );
        Button::new("version-badge")
            .flex()
            .items_center()
            .h(px(20.))
            .my(px(-2.))
            .px(px(8.))
            .border_1()
            .border_color(p.stroke)
            .rounded_full()
            .text_color(p.muted)
            .font_features(FontFeatures(Arc::new(vec![("tnum".into(), 1)])))
            .hover(move |s| s.bg(p.hover).text_color(p.ink))
            .active(move |s| s.bg(p.press))
            .accessibility_label(version_hint.clone())
            .tooltip(move |window, cx| widgets::tooltip(version_hint.clone(), window, cx))
            .child(tr!("shell.status.version", version = short))
            .when(self.options.test, |d| {
                d.child(anchor_probe("version-badge", self.ui.anchors.clone()))
            })
            .on_click(cx.listener(|this, _, window, cx| {
                this.open_dialog("whatsnew", json!({}), window, cx)
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.act(
                        Action::Copy(copied.clone(), "actions.copyWhat.version"),
                        window,
                        cx,
                    );
                }),
            )
            .into_any_element()
    }
    /// The version badge while an update waits: the download's progress
    /// fills it; ready to install, or a newer portable build out, it wears the
    /// accent. It opens the update; a right click offers to skip the version.
    fn update_badge(&self, updates: &Value, cx: &mut Context<Self>) -> AnyElement {
        let p = self.palette;
        let state = text(updates, "state");
        let version = text(updates, "version").to_owned();
        let total = number(updates, "total");
        let progress = if total > 0. {
            (number(updates, "received") / total).clamp(0., 1.)
        } else {
            0.
        };
        let (title, hint) = match state {
            "downloading" => (
                if total > 0. {
                    tr!(
                        "settings.update.status.downloadingPercent",
                        version = version,
                        percent = (progress * 100.).floor() as u64
                    )
                } else {
                    tr!("settings.update.status.downloading", version = version)
                },
                tr!("updates.button.downloadingTitle", version = version),
            ),
            "available" => (
                tr!("updates.badge.available", version = version),
                tr!("updates.button.availableTitle", version = version),
            ),
            _ => (
                tr!("updates.badge.ready", version = version),
                tr!("updates.button.title", version = version),
            ),
        };
        let filled = state != "downloading";
        let color = if filled { p.on_accent } else { p.ink };
        Button::new("update-status")
            .relative()
            .flex()
            .items_center()
            .gap(px(5.))
            .h(px(20.))
            .my(px(-2.))
            .px(px(8.))
            .rounded_full()
            .overflow_hidden()
            .text_color(color)
            .font_features(FontFeatures(Arc::new(vec![("tnum".into(), 1)])))
            .when(filled, |b| b.bg(p.accent).hover(|s| s.opacity(0.92)))
            .when(!filled, |b| {
                b.border_1()
                    .border_color(p.stroke)
                    .hover(move |s| s.bg(p.hover))
                    .child(
                        div()
                            .absolute()
                            .left_0()
                            .top_0()
                            .bottom_0()
                            .w(relative(progress as f32))
                            .bg(p.accent.opacity(0.22)),
                    )
            })
            .child(icon("UpdateIcon", color).size(px(12.)))
            .child(title)
            .accessibility_label(hint.clone())
            .tooltip(move |window, cx| widgets::tooltip(hint.clone(), window, cx))
            .on_click(cx.listener(|this, _, window, cx| {
                this.act(Action::Dialog("update".into(), json!({})), window, cx)
            }))
            .on_mouse_down(MouseButton::Right, {
                cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    let items = vec![
                        MenuItem::new(
                            label("updates.menu.view"),
                            Action::Dialog("update".into(), json!({})),
                        )
                        .icon("UpdateIcon"),
                        MenuItem::new(
                            label("updates.menu.dismiss"),
                            Action::Preference("updateDismissed".into(), json!(version)),
                        )
                        .icon("DismissIcon"),
                    ];
                    this.act(Action::Popup(items), window, cx);
                })
            })
            .into_any_element()
    }
    /// Sampling in the status bar: a press pauses or resumes every page, the
    /// reading beside it opens the refresh rate.
    fn refresh_control(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = self.palette;
        let paused = self.paused;
        let color = if paused { p.accent } else { p.muted };
        let pause = label(if paused {
            "shell.resume"
        } else {
            "shell.pause"
        });
        let pause_tip = format!("{pause}\n{}", label("shell.status.holdHint"));
        let rate = label("shell.interval.label");
        let reading = if paused {
            tr!("shell.status.paused")
        } else {
            tr!(
                "shell.status.interval",
                seconds = crate::regional::seconds(self.interval)
            )
        };
        div()
            .id("status-rate")
            .relative()
            .flex()
            .items_center()
            .h(px(20.))
            .my(px(-2.))
            .border_1()
            .border_color(if paused {
                p.accent.opacity(0.55)
            } else {
                p.stroke
            })
            .rounded_full()
            .overflow_hidden()
            .text_color(color)
            // The reading, then the pause button at the end: the button stays
            // where it is as the reading changes length with the state.
            .child(
                Button::new("status-interval")
                    .h_full()
                    .pl(px(7.))
                    .pr(px(6.))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .hover(move |s| s.bg(p.hover))
                    .active(move |s| s.bg(p.press))
                    .accessibility_label(rate.clone())
                    .child(div().size(px(6.)).rounded_full().bg(if paused {
                        p.faint
                    } else {
                        p.cpu
                    }))
                    .child(reading)
                    .child(icon("ChevronUp", color).size(px(12.)))
                    .tooltip(move |window, cx| widgets::tooltip(rate.clone(), window, cx))
                    .on_click(
                        cx.listener(|this, _, window, cx| this.open_interval_menu(window, cx)),
                    ),
            )
            .child(div().w(px(1.)).h_full().bg(p.stroke))
            .child(
                Button::new("status-pause")
                    .h_full()
                    .px(px(6.))
                    .flex()
                    .items_center()
                    .hover(move |s| s.bg(p.hover))
                    .active(move |s| s.bg(p.press))
                    .accessibility_label(pause)
                    .child(icon(if paused { "PlayIcon" } else { "PauseIcon" }, color).size(px(12.)))
                    .tooltip(move |window, cx| widgets::tooltip(pause_tip.clone(), window, cx))
                    .on_click(
                        cx.listener(|this, _, window, cx| this.act(Action::Pause, window, cx)),
                    ),
            )
            .child(anchor_probe(RATE, self.ui.anchors.clone()))
            .into_any_element()
    }
    fn open_interval_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // The menu ticks the current one (`open_popup_at`).
        let items = INTERVALS
            .iter()
            .map(|ms| {
                MenuItem::new(
                    tr!(
                        "shell.interval.option",
                        seconds = crate::regional::seconds(*ms)
                    ),
                    Action::Interval(*ms),
                )
            })
            .collect();
        self.open_anchored(items, RATE, Side::Above, window, cx);
    }
    /// While a splitter is dragged: follows the pointer and ends the drag on
    /// release wherever they happen. The listeners are the window's, in the
    /// capture phase: the root's own move and release listeners need its
    /// hitbox hovered, so a pane over it (the details pane occludes the list)
    /// swallowed them and a press left the splitter dragging after release.
    fn drag_capture(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let desktop = cx.entity().downgrade();
        canvas(
            |_, _, _| {},
            move |_, _, window, _| {
                let moving = desktop.clone();
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                    if phase == DispatchPhase::Capture {
                        let _ = moving.update(cx, |this, cx| this.mouse_move(event, window, cx));
                    }
                });
                let ending = desktop.clone();
                window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                    if phase == DispatchPhase::Capture && event.button == MouseButton::Left {
                        let _ = ending.update(cx, |this, cx| this.end_drag(cx));
                    }
                });
            },
        )
        .absolute()
        .size_0()
    }
    /// Ends a splitter drag and keeps the width it left.
    fn end_drag(&mut self, cx: &mut Context<Self>) {
        if let Some((kind, _, _)) = self.drag.take() {
            if kind == "details" {
                self.prefs.set("detailsWidth", self.details_width);
            } else if kind == "perf-sidebar" {
                self.prefs.set("perfSideWidth", self.perf_sidebar_width);
            } else if kind == "nav" {
                self.prefs.set("navWidth", self.nav_width);
            }
        }
        cx.notify();
    }
    pub(in crate::frontend) fn mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A release can come before the listeners above are painted (a click
        // within one frame); a move without the button means it came.
        if self.drag.is_some() && event.pressed_button != Some(MouseButton::Left) {
            self.end_drag(cx);
            return;
        }
        if let Some((kind, origin, start)) = &self.drag {
            if kind == "details" {
                self.details_width = Self::clamp_details_width(
                    *start - f32::from(event.position.x - origin.x),
                    window,
                );
                cx.notify();
            } else if kind == "perf-sidebar" {
                self.perf_sidebar_width =
                    (*start + f32::from(event.position.x - origin.x)).clamp(200., 440.);
                cx.notify();
            } else if kind == "nav" {
                self.nav_width =
                    Self::clamp_nav_width(*start + f32::from(event.position.x - origin.x));
                cx.notify();
            }
        }
    }
}
impl Render for Desktop {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let render_started = (self.options.test
            && std::env::var_os("ARGUS_GPUI_VIEW_TRACE").is_some())
        .then(Instant::now);
        self.render_count = self.render_count.wrapping_add(1);
        if std::mem::take(&mut self.page_enter_pending) {
            // As a CSS animation: it starts when first drawn, after the switch's work.
            self.page_switched = Instant::now();
            self.hold_entrance(window, cx);
        }
        let p = self.palette;
        let bar_level = self.fit_toolbar(window, cx);
        let bar_extra = self.bar_extra(bar_level);
        let toolbar = self.region(
            regions::RegionKind::Toolbar,
            StyleRefinement::default()
                .w_full()
                .h(theme::dimension("toolbarHeight"))
                .flex_shrink_0(),
            cx,
        );
        // Beside the side navigation the page bar heads the page: its row
        // (32 px and the 6 px gap below) and the controls' own row.
        let header = (!self.nav_top).then(|| {
            self.region(
                regions::RegionKind::Header,
                StyleRefinement::default()
                    .w_full()
                    .h(px(32. + 6.) + bar_extra)
                    .flex_shrink_0(),
                cx,
            )
        });
        // The page is a region: an animation inside the toolbar or a page
        // entrance replays it, and its own frames redraw only it.
        let content = self.region(
            regions::RegionKind::Page,
            StyleRefinement::default().size_full(),
            cx,
        );
        let footer = self.statusbar(cx);
        let body = div()
            .flex_1()
            .min_h_0()
            .flex()
            .when(!self.nav_top, |d| {
                let width = px(self.side_nav_width());
                d.child(self.region(
                    regions::RegionKind::Navigation,
                    StyleRefinement::default().w(width).h_full().flex_shrink_0(),
                    cx,
                ))
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .children(header)
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .child(self.enter(content, window, cx)),
                    )
                    .child(footer),
            );
        let overlays = self.render_overlays(window, cx);
        let composer = self.search_composer(window, cx);
        self.schedule_chart_tick(window, cx);
        let result = div()
            .id("argus-root")
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(p.background)
            .text_color(p.ink)
            .font_family(cx.theme().font_family.clone())
            .text_size(cx.theme().font_size)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .on_modifiers_changed(cx.listener(Self::modifiers_changed))
            .on_drop(cx.listener(Self::accept_dropped_files))
            // Before any text input, which then takes over from it.
            .children(composer)
            .when(self.drag.is_some(), |d| d.child(self.drag_capture(cx)))
            // First, so its press and wheel listeners precede the page's.
            .child(self.tip_layer.clone())
            .child(toolbar)
            .when(self.nav_top, |d| {
                // The tabs (32 px a row) and their 6 px gap below. With large
                // text or scale they wrap onto more rows (as last laid out).
                // (Just moved from the side, the bounds are the pane's.)
                let tabs = self.navigation_regions.borrow()[0].size;
                let tabs = if tabs.width > tabs.height {
                    tabs.height
                } else {
                    px(32.)
                };
                d.child(
                    self.region(
                        regions::RegionKind::Navigation,
                        StyleRefinement::default()
                            .w_full()
                            .h(tabs.max(px(32.)) + px(6.) + bar_extra)
                            .flex_shrink_0(),
                        cx,
                    ),
                )
            })
            .child(body)
            .children(overlays);
        if let Some(render_started) = render_started {
            println!(
                "{}",
                json!({"event":"ui-render-cost","micros":render_started.elapsed().as_micros(),"page":self.page})
            );
        }
        result
    }
}

fn track_region<const N: usize>(
    index: usize,
    regions: Rc<RefCell<[Bounds<Pixels>; N]>>,
    owner: WeakEntity<Desktop>,
) -> impl Fn(Bounds<Pixels>, &mut Window, &mut App) + 'static {
    move |bounds, window, cx| {
        let previous = regions.borrow()[index];
        if previous == bounds {
            return;
        }
        regions.borrow_mut()[index] = bounds;
        // The entries' container (the first region) only longer or shorter
        // along its row or column, as when the window is resized, moves none
        // of the entries (those that move are measured themselves): it is
        // noted for the next render rather than drawing the desktop again.
        let row = |b: Bounds<Pixels>| b.size.width > b.size.height;
        let stretched = index == 0
            && previous.origin == bounds.origin
            && row(previous) == row(bounds)
            && if row(bounds) {
                previous.size.height == bounds.size.height
            } else {
                previous.size.width == bounds.size.width
            };
        if stretched {
            return;
        }
        let owner = owner.clone();
        window.defer(cx, move |_, cx| {
            let _ = owner.update(cx, |_, cx| cx.notify());
        });
    }
}

/// Measures the title row's brand and the row itself (the search box sits
/// between them), and the search box for tests. When the brand or the row
/// moves, the toolbar alone draws again to place the search box: a desktop
/// notification would draw the whole page again, in the middle of its entrance.
fn track_title(
    index: usize,
    regions: Rc<RefCell<[Bounds<Pixels>; 3]>>,
    owner: WeakEntity<Desktop>,
) -> impl Fn(Bounds<Pixels>, &mut Window, &mut App) + 'static {
    move |bounds, window, cx| {
        if regions.borrow()[index] == bounds {
            return;
        }
        regions.borrow_mut()[index] = bounds;
        if index == 1 {
            return;
        }
        let owner = owner.clone();
        window.defer(cx, move |_, cx| {
            let _ = owner.update(cx, |this, cx| {
                this.refresh_region(regions::RegionKind::Toolbar, cx)
            });
        });
    }
}

/// Measures the page bar's end controls and the bar (`fit_toolbar`), and its
/// drag stretch for tests. Another page's controls move them on its first
/// frame: the bar steps to another level (and the desktop draws again) only
/// if it no longer fits at the level drawn. While the bar is `settling`, a
/// frame at a `level` that does not fit is only measured: the controls never
/// show at the end of the row and then jump below it.
fn track_bar(
    index: usize,
    regions: Rc<RefCell<[Bounds<Pixels>; 3]>>,
    owner: WeakEntity<Desktop>,
    settling: Rc<Cell<bool>>,
    level: u8,
) -> impl Fn(Bounds<Pixels>, &mut Window, &mut App) + 'static {
    move |bounds, window, cx| {
        if regions.borrow()[index] == bounds {
            return;
        }
        regions.borrow_mut()[index] = bounds;
        if index == 2 {
            return;
        }
        if settling.get() && level < 2 {
            let [trailing, bar, _] = *regions.borrow();
            if trailing.size.width > px(0.)
                && bar.size.width > px(0.)
                && trailing.right() - (bar.right() - px(BAR_INSET)) > px(0.5)
            {
                window.defer_presentation_until_layout();
            }
        }
        let owner = owner.clone();
        window.defer(cx, move |window, cx| {
            let _ = owner.update(cx, |this, cx| {
                this.fit_toolbar(window, cx);
            });
        });
    }
}

/// Records an element's bounds under `key`: a menu it opens hangs from them,
/// and tests reach it there (`clickAnchor`).
pub(in crate::frontend) fn anchor_probe(
    key: impl Into<SharedString>,
    anchors: Rc<RefCell<HashMap<SharedString, Bounds<Pixels>>>>,
) -> impl IntoElement {
    let key = key.into();
    canvas(
        move |bounds, _, _| {
            anchors.borrow_mut().insert(key, bounds);
        },
        |_, _, _, _| {},
    )
    .absolute()
    .inset_0()
}

/// Up time in the status bar, in words: days and hours, hours and minutes, or minutes and seconds.
pub(in crate::frontend) fn status_uptime(seconds: f64) -> String {
    let s = seconds as u64;
    let (days, hours, minutes) = (s / 86400, s % 86400 / 3600, s % 3600 / 60);
    if days > 0 {
        tr!("format.uptime.days", days = days, hours = hours)
    } else if hours > 0 {
        tr!("format.uptime.hours", hours = hours, minutes = minutes)
    } else {
        tr!("format.uptime.minutes", minutes = minutes, seconds = s % 60)
    }
}
