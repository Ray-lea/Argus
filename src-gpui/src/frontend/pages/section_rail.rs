//! The list of a page's sections on its left (Settings, Components): it
//! marks the section in view as the page scrolls, and a click scrolls
//! smoothly to a section. It shows where the page leaves room for it beside
//! the column of cards.
use super::*;
use gpui_kit::base::ElementExt;

const MIN_RAIL: f32 = 180.;
const NAV_LEFT: f32 = 14.;
const NAV_RIGHT: f32 = 12.;
const NAV_GAP: f32 = 10.;
const DOT: f32 = 6.;
const GAP: f32 = 32.;
/// The column of cards beside the list: this wide while the page has the
/// room, then wider by half of the room beyond, up to `WIDEST`.
const CONTENT: f32 = 780.;
const WIDEST: f32 = 960.;
/// The page's width at most that leaves no room for a list of the shortest
/// names.
const NARROW: f32 = 900.;

/// A page's sections, the scroll they follow and the one marked in the list.
pub(in crate::frontend) struct SectionRail {
    pub scroll: ScrollHandle,
    /// The section marked in the list.
    pub current: usize,
    /// A section to show once the page is laid out (opening it there).
    target: Option<usize>,
    /// The page's width, as last laid out.
    width: Rc<Cell<Pixels>>,
    /// A section was picked in the list or opened: it stays marked until the
    /// page is scrolled by hand.
    picked: bool,
    /// The target was set before the last layout and waits for the next.
    jump_wait: bool,
    /// The smooth scroll to a picked section: from, to and its start.
    glide: Option<(Pixels, Pixels, Instant)>,
}

impl Default for SectionRail {
    fn default() -> Self {
        Self {
            scroll: ScrollHandle::new(),
            current: 0,
            target: None,
            width: Rc::new(Cell::new(px(0.))),
            picked: false,
            jump_wait: false,
            glide: None,
        }
    }
}

/// An entry of the list.
pub(super) struct RailEntry {
    /// The section's key: the entry's id and test anchor `<page>-nav-<key>`.
    pub key: &'static str,
    pub name: String,
    pub icon: &'static str,
    /// An accent dot after the name: none for an entry that never shows one,
    /// or whether it shows (room is kept for it either way).
    pub dot: Option<bool>,
}

impl SectionRail {
    /// Shows `index` when the page is next laid out, marking it.
    pub(in crate::frontend) fn open_at(&mut self, index: usize) {
        self.current = index;
        self.target = Some(index);
    }

    /// Moves the mark with the scroll, unless a section was just picked;
    /// shows a section opened, and plays a picked section's smooth scroll.
    fn follow(&mut self, count: usize, window: &mut Window, cx: &App) {
        let handle = self.scroll.clone();
        let container = handle.bounds();
        if let Some(index) = self.target.take() {
            // Wait for one layout after the request: the page may be arriving,
            // or its text may have reflowed (a language change) since the last.
            let fresh = std::mem::replace(&mut self.jump_wait, false);
            self.current = index;
            self.picked = true;
            match handle.bounds_for_item(index) {
                Some(child) if fresh && container.size.height > px(0.) => {
                    let target = (container.top() - child.top())
                        .max(-handle.max_offset().y)
                        .min(px(0.));
                    handle.set_offset(point(px(0.), target));
                    self.glide = None;
                }
                _ => {
                    self.target = Some(index);
                    self.jump_wait = true;
                    window.request_animation_frame();
                }
            }
        }
        if let Some((from, to, start)) = self.glide {
            let t = (start.elapsed().as_secs_f32() / 0.3).min(1.);
            let y = from
                + (to - from)
                    * if cx.reduce_motion() {
                        1.
                    } else {
                        theme::ease_out(t)
                    };
            handle.set_offset(point(px(0.), y));
            if t >= 1. || cx.reduce_motion() {
                self.glide = None;
            } else {
                window.request_animation_frame();
            }
        }
        if self.picked || container.size.height <= px(0.) {
            return;
        }
        let h = f32::from(container.size.height);
        let y = -f32::from(handle.offset().y);
        let max = f32::from(handle.max_offset().y);
        let start = (y / (h / 2.)).min(1.);
        let end = (1. - (max - y) / (h / 2.)).max(0.);
        let line = h * (0.1 + 0.25 * start + 0.6 * end);
        let mut current = 0;
        for index in 0..count {
            if let Some(child) = handle.bounds_for_item(index)
                && f32::from(child.top() - container.top()) - y <= line
            {
                current = index;
            }
        }
        if y >= max - 4. && max > 0. {
            current = count - 1;
        }
        self.current = current;
    }

    /// Scrolls smoothly to a section picked in the list; it stays marked
    /// until the page is scrolled by hand.
    fn jump(&mut self, index: usize, cx: &App) {
        self.current = index;
        self.picked = true;
        let handle = &self.scroll;
        let Some(child) = handle.bounds_for_item(index) else {
            return;
        };
        let container = handle.bounds();
        let target = (container.top() - child.top())
            .max(-handle.max_offset().y)
            .min(px(0.));
        if cx.reduce_motion() {
            handle.set_offset(point(px(0.), target));
        } else {
            self.glide = Some((handle.offset().y, target, Instant::now()));
        }
    }
}

impl Desktop {
    /// The section marked in the settings' and the components' lists, and
    /// the left and the width of each page's cards as last drawn (test only).
    pub(in crate::frontend) fn section_rail_state(&self) -> Value {
        let anchors = self.ui.anchors.borrow();
        let cards = |page: &str| {
            anchors
                .get(format!("{page}-cards").as_str())
                .map(|bounds| [f32::from(bounds.left()), f32::from(bounds.size.width)])
        };
        json!({
            "settings": super::settings::SECTIONS.get(self.settings_rail.current),
            "components": super::components::SECTIONS.get(self.components_rail.current),
            "cards": { "settings": cards("settings"), "components": cards("components") },
        })
    }

    /// A page of sections in a scrolled column, the list of them on its left
    /// where the page leaves room for it. `page` names the page's elements
    /// (`<page>-content`, `<page>-nav-<key>`); `rail` is where its state
    /// lives; `sections` are in the order of `entries`.
    pub(super) fn section_page(
        &mut self,
        page: &'static str,
        rail: fn(&mut Desktop) -> &mut SectionRail,
        entries: Vec<RailEntry>,
        sections: Vec<AnyElement>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let count = entries.len();
        {
            let state = rail(self);
            if state.current >= count {
                state.current = 0;
            }
            state.follow(count, window, cx);
        }
        let p = self.palette;
        let state = rail(self);
        let (scroll, selected, measured) =
            (state.scroll.clone(), state.current, state.width.clone());
        let width = f32::from(measured.get());
        let width = if width > 0. {
            width
        } else {
            f32::from(window.viewport_size().width) - 24.
        };
        // The gutter is kept (`scrollbar-gutter: stable`): the container query
        // and the centring see the width beside it.
        let width = width - scrollbar::SIZE;
        let controls = NAV_LEFT + f32::from(window.rem_size()) + NAV_GAP + NAV_RIGHT;
        let rail_width = entries.iter().fold(MIN_RAIL, |width, entry| {
            let text = theme::text_width(&entry.name, theme::font(13.), window, cx);
            let dot = if entry.dot.is_some() {
                NAV_GAP + DOT
            } else {
                0.
            };
            width.max(f32::from(text.ceil()) + controls + dot)
        });
        // Only whether the list fits beside the cards follows the measured
        // width; the column itself is sized by layout, so that it follows the
        // window in the frame it is resized in (laid out from a measurement,
        // every step of a slow resize drew one frame at the previous width
        // and the text wrapped back and forth).
        let threshold = NARROW + (rail_width - MIN_RAIL);
        let narrow = width <= threshold;
        let least = rail_width + GAP + CONTENT;
        let lead = if narrow { 0. } else { rail_width + GAP };
        let most = if narrow { least } else { lead + WIDEST };
        // A row 4 px in from the page's sides holding one column: the page's
        // width up to `least`, then half of the room beyond, up to `most`;
        // the margins either side share the rest, so that it stays centred.
        // The sections, and the list over them.
        let margin = || div().flex_grow(0.5).flex_basis(px(0.));
        let framed = |column: Div| {
            div()
                .w_full()
                .flex()
                .child(margin())
                .child(
                    column
                        .flex_grow(1.)
                        .flex_basis(px(least))
                        .min_w_0()
                        .max_w(px(most)),
                )
                .child(margin())
        };
        let (test, anchors) = (self.options.test, self.ui.anchors.clone());
        let sections = sections.into_iter().enumerate().map(|(index, section)| {
            // Where tests read the cards' column: the first section's.
            let section = if test && index == 0 {
                let anchors = anchors.clone();
                let anchor = SharedString::from(format!("{page}-cards"));
                div()
                    .relative()
                    .child(section)
                    .child(
                        canvas(
                            move |bounds, _, _| {
                                anchors.borrow_mut().insert(anchor.clone(), bounds);
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .inset_0(),
                    )
                    .into_any_element()
            } else {
                section
            };
            framed(div().pl(px(lead)).child(section)).flex_shrink_0()
        });
        let body = div()
            .id(SharedString::from(format!("{page}-content")))
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&scroll)
            .flex()
            .flex_col()
            .items_center()
            .pl(px(4.))
            .pr(px(4. + scrollbar::SIZE))
            .pt(px(4.))
            .pb(px(32.))
            .children(sections)
            .on_scroll(cx.listener(move |this, _, _, _| rail(this).picked = false))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, _| rail(this).picked = false),
            )
            .child(scrollbar::scrollbar(
                SharedString::from(format!("{page}-scrollbar")),
                &scroll,
                p.dark,
            ));
        let mark = gpui_kit::base::motion::transition(
            format!("{page}-nav-mark"),
            selected as f32 * 38.,
            theme::fluent(250, theme::ease_move),
            window,
            cx,
        );
        let list = div()
            .absolute()
            .top(px(4.))
            .left_0()
            .w(px(rail_width))
            .flex()
            .flex_col()
            .gap(px(2.))
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(px(mark))
                    .h(px(36.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(p.stroke)
                    .bg(p.layer_strong)
                    .child(
                        div()
                            .absolute()
                            .left_0()
                            .top(px(8.))
                            .w(px(3.))
                            .h(px(18.))
                            .rounded(px(2.))
                            .bg(p.accent),
                    ),
            )
            .children(entries.into_iter().enumerate().map(|(index, entry)| {
                let current = index == selected;
                let control = SharedString::from(format!("{page}-nav-{}", entry.key));
                gpui_kit::base::Button::new(control.clone())
                    .relative()
                    .h(px(36.))
                    .pl(px(NAV_LEFT))
                    .pr(px(NAV_RIGHT))
                    .gap(px(NAV_GAP))
                    .line_height(theme::line_height())
                    .justify_start()
                    .rounded(px(6.))
                    .text_color(if current { p.ink } else { p.muted })
                    .when(!current, |b| {
                        b.hover(move |s| s.bg(p.hover).text_color(p.ink))
                    })
                    .role(Role::Tab)
                    .accessibility_label(entry.name.clone())
                    .child(self.moving_icon(&control, entry.icon, None, None))
                    // Only an entry that changes under the pointer draws again for it.
                    .when(!current, |b| b.child(self.icon_probe(&control)))
                    .child(div().whitespace_nowrap().child(entry.name))
                    .when(entry.dot == Some(true), |b| {
                        b.child(div().ml_auto().size(px(DOT)).rounded_full().bg(p.accent))
                    })
                    // Where tests take hold of it.
                    .when(test, |b| {
                        let anchors = anchors.clone();
                        let anchor = control.clone();
                        b.child(
                            canvas(
                                move |bounds, _, _| {
                                    anchors.borrow_mut().insert(anchor.clone(), bounds);
                                },
                                |_, _, _, _| {},
                            )
                            .absolute()
                            .inset_0(),
                        )
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        rail(this).jump(index, cx);
                        cx.notify();
                    }))
                    // The list lies over the cards: a press here is not one in
                    // them, which would hand the mark back to the scroll before
                    // the click and send it down to the last section first.
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            }));
        // Over the scrolled column, in the same place as its sections.
        let list = div()
            .absolute()
            .inset_0()
            .pl(px(4.))
            .pr(px(4. + scrollbar::SIZE))
            .child(framed(div().relative().child(list)));
        let owner = cx.entity().downgrade();
        div()
            .size_full()
            .min_h_0()
            .px(px(12.))
            .child(
                div()
                    .relative()
                    .size_full()
                    .on_prepaint(move |bounds, window, cx| {
                        let width = bounds.size.width;
                        if measured.replace(width) == width {
                            return;
                        }
                        // Drawn again only when the list moves in or out.
                        if (f32::from(width) - scrollbar::SIZE <= threshold) != narrow {
                            window.defer(cx, move |_, cx| {
                                let _ = owner.update(cx, |_, cx| cx.notify());
                            });
                        }
                    })
                    .child(body)
                    .when(!narrow, |d| d.child(list)),
            )
            .into_any_element()
    }
}
