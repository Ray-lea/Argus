//! Context menu geometry, entries and keyboard/pointer interaction.
use super::*;

/// `.menu-item`: row height, and its padding and the gap between icon, label and hint.
const ROW: f32 = 30.;
const GAP: f32 = 10.;
const ICON: f32 = 16.;
/// The submenu arrow (`ChevronRight size={12}`).
const CHEVRON: f32 = 12.;
/// `.menu`: padding inside its 1 px border, and its minimum width.
const INSET: f32 = 4.;
const MIN_WIDTH: f32 = 220.;
/// `.menu-sep`: a 1 px line with 4 px above and below.
const SEPARATOR: f32 = 9.;
/// Room kept from the window's edges.
const MARGIN: f32 = 8.;
/// Hover time before a submenu opens or another item closes it (Windows uses about 400 ms).
const SUBMENU_DELAY: Duration = Duration::from_millis(250);
/// `flyout-in`, and how long a closed surface takes to roll up.
const ENTER_SECONDS: f32 = 0.167;
const LEAVE_SECONDS: f32 = 0.083;
/// The scale `flyout-in` starts from. The menu position depends on its size
/// at that moment, so one that opens up or left of the pointer overlaps it
/// by the difference once grown; menus here are placed the same way.
const ENTER_SCALE: f32 = 0.96;

/// Where the keyboard is in an open menu. The first item
/// takes it when the menu opens, and the highlight shows once a key moves it
/// (`:focus-visible`).
#[derive(Clone, Copy, PartialEq)]
enum MenuFocus {
    None,
    Top(usize),
    Sub(usize),
}

/// An entry of the menu itself or of its open submenu.
#[derive(Clone, Copy, PartialEq)]
enum Entry {
    Top(usize),
    Sub(usize),
}

struct Submenu {
    /// The item of the menu that owns it.
    owner: usize,
    opened: Instant,
}

/// A surface of a menu.
#[derive(Clone, Copy, PartialEq)]
enum Part {
    Menu,
    Submenu,
    /// The submenu that closed or gave way to another, rolling up.
    Closed,
}

/// An open context menu.
pub(super) struct Popup {
    items: Vec<MenuItem>,
    /// The pointer when it opened: the menu grows from there toward free space.
    position: Point<Pixels>,
    /// The window's size then; resizing the window closes the menu.
    viewport: Size<Pixels>,
    opened: Instant,
    focus_handle: FocusHandle,
    focus: MenuFocus,
    focus_visible: bool,
    hovered: Option<Entry>,
    sub: Option<Submenu>,
    /// The submenu that closed or gave way to another, and when: it rolls
    /// up as the menu itself does.
    sub_leaving: Option<(Submenu, Instant)>,
    /// The pending hover change of the submenu.
    timer: Option<Task<()>>,
}

impl Popup {
    fn children(&self) -> &[MenuItem] {
        self.sub
            .as_ref()
            .and_then(|sub| self.items.get(sub.owner))
            .map_or(&[], |owner| &owner.children)
    }
    fn item(&self, entry: Entry) -> Option<&MenuItem> {
        match entry {
            Entry::Top(index) => self.items.get(index),
            Entry::Sub(index) => self.children().get(index),
        }
    }
    /// Whether the keyboard is in the submenu.
    fn in_sub(&self) -> bool {
        matches!(self.focus, MenuFocus::Sub(_)) && self.sub.is_some()
    }
}

/// Items that take the keyboard and clicks.
fn enabled(items: &[MenuItem]) -> impl Iterator<Item = usize> + '_ {
    items
        .iter()
        .enumerate()
        .filter(|(_, item)| !item.separator && !item.disabled)
        .map(|(index, _)| index)
}

/// Separators only between items, one at a time.
fn tidy(items: Vec<MenuItem>) -> Vec<MenuItem> {
    let mut out: Vec<MenuItem> = Vec::with_capacity(items.len());
    for mut item in items {
        if item.separator && out.last().is_none_or(|last| last.separator) {
            continue;
        }
        item.children = tidy(std::mem::take(&mut item.children));
        out.push(item);
    }
    if out.last().is_some_and(|last| last.separator) {
        out.pop();
    }
    out
}

/// Flips a staying check where it is drawn; an owner showing a count of its
/// checked entries counts again.
fn toggle_in_place(popup: &mut Popup, entry: Entry) {
    match entry {
        Entry::Top(index) => {
            if let Some(item) = popup.items.get_mut(index) {
                item.checked = !item.checked;
            }
        }
        Entry::Sub(index) => {
            let Some(owner) = popup
                .sub
                .as_ref()
                .and_then(|sub| popup.items.get_mut(sub.owner))
            else {
                return;
            };
            if let Some(item) = owner.children.get_mut(index) {
                item.checked = !item.checked;
            }
            if owner.detail.is_some() {
                let entries = owner.children.iter().filter(|item| !item.separator);
                let checked = entries.clone().filter(|item| item.checked).count();
                owner.detail = Some(format!("{checked}/{}", entries.count()));
            }
        }
    }
}

/// How much shows of a surface that opened at `since` (`flyout-in`).
fn entered(since: Instant, reduce: bool) -> f32 {
    if reduce {
        1.
    } else {
        theme::ease_out((since.elapsed().as_secs_f32() / ENTER_SECONDS).min(1.))
    }
}

/// How much is left of a surface that closed at `since`, rolling up at an
/// even pace: nothing once that is through, or at once under reduced motion.
fn remaining(since: Instant, reduce: bool) -> Option<f32> {
    let left = 1. - since.elapsed().as_secs_f32() / LEAVE_SECONDS;
    (!reduce && left > 0.).then_some(left)
}

/// A closed surface on its way out: what `left` of its height shows, from the
/// edge it opened at (its bottom if `upward`), drawn where it was. `surface`
/// draws it at an origin.
///
/// It rolls up rather than fades out. Over the window's backdrop a fading
/// surface is translucent all over, and some displays show such pixels
/// brighter than either the surface or the page: the menu flashed as it
/// closed. A clip leaves every pixel either the surface's or the page's
/// (`docs/design/window.md`).
fn rolled(
    origin: Point<Pixels>,
    size: Size<Pixels>,
    left: f32,
    upward: bool,
    surface: impl FnOnce(Point<Pixels>) -> Stateful<Div>,
) -> AnyElement {
    if left >= 1. {
        return surface(origin).into_any_element();
    }
    let height = size.height * left.max(0.);
    let top = if upward {
        origin.y + size.height - height
    } else {
        origin.y
    };
    div()
        .absolute()
        .left(origin.x)
        .top(top)
        .w(size.width)
        .h(height)
        .overflow_hidden()
        .child(surface(point(px(0.), origin.y - top)))
        .into_any_element()
}

/// Whether a menu is open, and when the last one closed.
#[derive(Default)]
struct Presence {
    open: bool,
    closed: Option<Instant>,
}

impl Global for Presence {}

/// Whether a menu is open or still rolling up. No hover hint shows meanwhile
/// (`widgets::tooltip`): a hint's delay and the element it belongs to know
/// nothing of a menu opened over them, and the hint would show on top of it.
pub(super) fn in_the_way(cx: &App) -> bool {
    cx.try_global::<Presence>().is_some_and(|presence| {
        presence.open
            || presence
                .closed
                .is_some_and(|at| remaining(at, cx.reduce_motion()).is_some())
    })
}

/// Whether a menu of `rows` items fits a window `height` tall. A menu does
/// not scroll: a list that may outgrow the window is checked before it is
/// offered.
pub(super) fn fits(rows: usize, height: Pixels) -> bool {
    px(ROW * rows as f32 + 2. * (INSET + 1. + MARGIN)) <= height
}

/// A menu's border box and where each entry starts in it.
struct MenuLayout {
    size: Size<Pixels>,
    tops: Vec<f32>,
}

/// The menu's size: as wide as its widest row
/// (no narrower than 220 px), 30 px per item and 9 px per separator.
fn measure(items: &[MenuItem], window: &Window, cx: &App) -> MenuLayout {
    let text = theme::font(13.);
    let hint = theme::font(12.);
    let width = |s: &str, size| f32::from(theme::text_width(s, size, window, cx).ceil());
    let mut widest = MIN_WIDTH;
    let mut y = 1. + INSET;
    let mut tops = Vec::with_capacity(items.len());
    for item in items {
        tops.push(y);
        if item.separator {
            y += SEPARATOR;
            continue;
        }
        let mut row = GAP + ICON + GAP + width(&item.label, text) + GAP;
        if let Some(hint_text) = item.hint {
            row += GAP + width(hint_text, hint);
        }
        if let Some(detail) = &item.detail {
            row += GAP + width(detail, hint);
        }
        if !item.children.is_empty() {
            row += GAP + CHEVRON;
        }
        widest = widest.max(row + 2. * (INSET + 1.));
        y += ROW;
    }
    MenuLayout {
        size: size(px(widest), px(y + INSET + 1.)),
        tops,
    }
}

/// The menu at the pointer, opening toward the free side as Windows menus do.
/// `menu` is its size as placed (`ENTER_SCALE`).
fn menu_origin(at: Point<Pixels>, menu: Size<Pixels>, viewport: Size<Pixels>) -> Point<Pixels> {
    let margin = px(MARGIN);
    let x = if at.x + menu.width > viewport.width - margin {
        (at.x - menu.width).max(margin)
    } else {
        at.x
    };
    let y = if at.y + menu.height > viewport.height - margin {
        (at.y - menu.height).max(margin)
    } else {
        at.y
    };
    point(x.max(px(0.)), y.max(px(0.)))
}

/// A submenu beside its owner, toward the free side, its first item level
/// with the owner. `sub` is its size as placed (`ENTER_SCALE`).
fn submenu_origin(
    menu: Point<Pixels>,
    menu_size: Size<Pixels>,
    owner_top: f32,
    sub: Size<Pixels>,
    viewport: Size<Pixels>,
) -> Point<Pixels> {
    let margin = px(MARGIN);
    let left = menu.x + px(1. + INSET);
    let right = menu.x + menu_size.width - px(1. + INSET);
    let top = menu.y + px(owner_top);
    let x = if right + sub.width > viewport.width - margin {
        (left - sub.width + px(4.)).max(margin)
    } else {
        right - px(4.)
    };
    let y = (top - px(5.))
        .min(viewport.height - margin - sub.height)
        .max(margin);
    point(x, y)
}

impl Desktop {
    /// Opens `items` at the pointer, taking the keyboard.
    pub(in crate::frontend) fn open_popup(
        &mut self,
        items: Vec<MenuItem>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let at = window.mouse_position();
        self.open_popup_at(items, at, window, cx);
    }

    /// Opens `items` at `at`, taking the keyboard.
    pub(in crate::frontend) fn open_popup_at(
        &mut self,
        items: Vec<MenuItem>,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let interval = self.interval;
        let items = tidy(
            items
                .into_iter()
                .map(|item| {
                    let checked = item.checked
                        || matches!(item.action, Action::Interval(ms) if ms == interval);
                    item.checked(checked)
                })
                .collect(),
        );
        let focus_handle = cx.focus_handle();
        focus_handle.focus(window, cx);
        // A right press on a table row also opens the table's own (empty)
        // menu, which takes the keyboard once this press is through: take it back.
        let handle = focus_handle.clone();
        cx.defer_in(window, move |this, window, cx| {
            if this
                .popup
                .as_ref()
                .is_some_and(|popup| popup.focus_handle == handle)
            {
                handle.focus(window, cx);
            }
        });
        let first = enabled(&items).next();
        self.popup = Some(Popup {
            focus: first.map_or(MenuFocus::None, MenuFocus::Top),
            items,
            position: at,
            viewport: window.viewport_size(),
            opened: Instant::now(),
            focus_handle,
            focus_visible: false,
            hovered: None,
            sub: None,
            sub_leaving: None,
            timer: None,
        });
        cx.set_global(Presence {
            open: true,
            closed: None,
        });
        window.refresh();
        cx.notify();
    }

    /// Drops the menu at once, as its page goes.
    pub(in crate::frontend) fn drop_menu(&mut self, cx: &mut Context<Self>) {
        if self.popup.take().is_some() {
            cx.set_global(Presence::default());
        }
    }

    /// Replaces the open menu's entries with others of the same arrangement,
    /// after a staying item changed what the rest say or allow. The keyboard
    /// and an open submenu stay where they are.
    pub(in crate::frontend) fn update_popup(&mut self, items: Vec<MenuItem>) {
        let items = tidy(items);
        if let Some(popup) = &mut self.popup
            && popup.items.len() == items.len()
        {
            popup.items = items;
        }
    }

    /// The open menu as tests see it: whether it has the keyboard, the item
    /// the keyboard is on, and the open submenu (test control `inspectMenu`).
    pub(in crate::frontend) fn menu_state(&self, window: &Window) -> Value {
        let Some(popup) = &self.popup else {
            return json!({"open": false});
        };
        let item = |items: &[MenuItem], index: usize| {
            items
                .get(index)
                .map(|item| json!({"label": item.label, "checked": item.checked}))
        };
        let (place, index) = match popup.focus {
            MenuFocus::None => ("none", None),
            MenuFocus::Top(index) => ("menu", Some(index)),
            MenuFocus::Sub(index) => ("submenu", Some(index)),
        };
        let focused = index.and_then(|index| match popup.focus {
            MenuFocus::Sub(_) => item(popup.children(), index),
            _ => item(&popup.items, index),
        });
        json!({
            "open": true,
            "keyboard": popup.focus_handle.is_focused(window),
            "focus": {"in": place, "index": index, "item": focused},
            "submenu": popup.sub.as_ref().map(|sub| sub.owner),
            "items": popup.items.iter().map(|item| &item.label).collect::<Vec<_>>(),
            "disabled": popup.items.iter().map(|item| item.disabled).collect::<Vec<_>>(),
        })
    }

    /// Closes the menu, which rolls up. The keyboard goes back to the window
    /// unless an item gave it to something else.
    pub(in crate::frontend) fn close_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mut popup) = self.popup.take() else {
            return;
        };
        popup.timer = None;
        if popup.focus_handle.contains_focused(window, cx) {
            self.focus.focus(window, cx);
        }
        let now = Instant::now();
        self.popup_leaving = Some((popup, now));
        cx.set_global(Presence {
            open: false,
            closed: Some(now),
        });
        // The rows re-sort once the input that closed the menu is through: a
        // press on another row must still find the rows it was aimed at.
        cx.spawn_in(window, async move |this, cx| {
            let _ = this.update_in(cx, |this, _, cx| {
                this.rebuild_rows(
                    this.pointer_in_table || this.modal.is_some() || this.popup.is_some(),
                    cx,
                );
                cx.notify();
            });
        })
        .detach();
        window.refresh();
        cx.notify();
    }

    /// The window going to the background or changing size closes the menu,
    /// as the interface's did on `blur` and `resize`.
    pub(in crate::frontend) fn watch_menu_window(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.subscriptions
            .push(cx.observe_window_activation(window, |this, window, cx| {
                if !window.is_window_active() {
                    this.close_menu(window, cx);
                }
            }));
        self.subscriptions
            .push(cx.observe_window_bounds(window, |this, window, cx| {
                if this
                    .popup
                    .as_ref()
                    .is_some_and(|popup| popup.viewport != window.viewport_size())
                {
                    this.close_menu(window, cx);
                }
            }));
    }

    /// Opens the submenu of the menu's item `owner`, moving the keyboard into it if `focus`.
    fn open_submenu(&mut self, owner: usize, focus: bool) {
        let Some(popup) = &mut self.popup else {
            return;
        };
        if popup.sub.as_ref().is_none_or(|sub| sub.owner != owner) {
            let now = Instant::now();
            let opened = Submenu { owner, opened: now };
            // Another item's submenu gives way to this one.
            if let Some(closed) = popup.sub.replace(opened) {
                popup.sub_leaving = Some((closed, now));
            }
        }
        let first = enabled(popup.children()).next();
        if focus && let Some(first) = first {
            popup.focus = MenuFocus::Sub(first);
        }
    }

    /// Closes the submenu; `refocus` gives the keyboard back to its owner.
    fn close_submenu(&mut self, refocus: bool) {
        let Some(popup) = &mut self.popup else {
            return;
        };
        let closed = popup.sub.take();
        let owner = closed.as_ref().map(|sub| sub.owner);
        if let Some(closed) = closed {
            popup.sub_leaving = Some((closed, Instant::now()));
        }
        popup.hovered = popup.hovered.filter(|entry| matches!(entry, Entry::Top(_)));
        match (refocus, owner) {
            (true, Some(owner)) => popup.focus = MenuFocus::Top(owner),
            // The item that had the keyboard is gone.
            _ if matches!(popup.focus, MenuFocus::Sub(_)) => popup.focus = MenuFocus::None,
            _ => {}
        }
    }

    /// Opens or closes the submenu after the hover delay.
    fn submenu_later(&mut self, open: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let task = cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(SUBMENU_DELAY).await;
            let _ = this.update_in(cx, |this, window, cx| {
                // This task is the one running: let it finish rather than cancel it.
                if let Some(task) = this.popup.as_mut().and_then(|popup| popup.timer.take()) {
                    task.detach();
                }
                match open {
                    Some(owner) => this.open_submenu(owner, false),
                    None => this.close_submenu(false),
                }
                window.refresh();
                cx.notify();
            });
        });
        if let Some(popup) = &mut self.popup {
            popup.timer = Some(task);
        }
    }

    /// The pointer entering or leaving an entry.
    fn menu_hover(
        &mut self,
        entry: Entry,
        hovered: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(popup) = &mut self.popup else {
            return;
        };
        if hovered {
            popup.hovered = Some(entry);
        } else if popup.hovered == Some(entry) {
            popup.hovered = None;
        }
        // Only the menu's own items open and close the submenu.
        if hovered && let Entry::Top(index) = entry {
            popup.timer = None;
            let owner = !popup.items[index].children.is_empty();
            let open = popup.sub.is_some();
            if owner {
                self.submenu_later(Some(index), window, cx);
            } else if open {
                self.submenu_later(None, window, cx);
            }
        }
        window.refresh();
        cx.notify();
    }

    /// A click on an entry, or Enter or Space on the focused one.
    fn menu_activate(&mut self, entry: Entry, window: &mut Window, cx: &mut Context<Self>) {
        let Some(popup) = &mut self.popup else {
            return;
        };
        let Some((owner, action, stay)) = popup
            .item(entry)
            .filter(|item| !item.disabled && !item.separator)
            .map(|item| (!item.children.is_empty(), item.action.clone(), item.stay))
        else {
            return;
        };
        popup.timer = None;
        if stay {
            toggle_in_place(popup, entry);
            self.act(action, window, cx);
            window.refresh();
            cx.notify();
            return;
        }
        if let Entry::Top(index) = entry
            && owner
        {
            self.open_submenu(index, true);
            window.refresh();
            cx.notify();
            return;
        }
        self.close_menu(window, cx);
        self.act(action, window, cx);
    }

    /// Keys while a menu is open; it takes them all, as the interface's menu
    /// blocked the window's shortcuts.
    pub(in crate::frontend) fn menu_key(
        &mut self,
        key: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(popup) = &mut self.popup else {
            return;
        };
        let in_sub = popup.in_sub();
        match key {
            "escape" if in_sub => {
                popup.timer = None;
                self.close_submenu(true);
            }
            "escape" => {
                self.close_menu(window, cx);
                return;
            }
            "left" if in_sub => {
                popup.timer = None;
                self.close_submenu(true);
            }
            "right" | "enter" | "space" => {
                let entry = match popup.focus {
                    MenuFocus::Top(index) => Entry::Top(index),
                    MenuFocus::Sub(index) if in_sub => Entry::Sub(index),
                    _ => return,
                };
                let owner = popup
                    .item(entry)
                    .is_some_and(|item| !item.children.is_empty());
                if key == "right" && !owner {
                    return;
                }
                popup.focus_visible = true;
                self.menu_activate(entry, window, cx);
                return;
            }
            "down" | "up" => {
                let (list, current) = if in_sub {
                    let current = match popup.focus {
                        MenuFocus::Sub(index) => Some(index),
                        _ => None,
                    };
                    (enabled(popup.children()).collect::<Vec<_>>(), current)
                } else {
                    let current = match popup.focus {
                        MenuFocus::Top(index) => Some(index),
                        _ => None,
                    };
                    (enabled(&popup.items).collect::<Vec<_>>(), current)
                };
                if list.is_empty() {
                    return;
                }
                let len = list.len() as isize;
                let at = current
                    .and_then(|current| list.iter().position(|index| *index == current))
                    .map_or(-1, |at| at as isize);
                let next = if key == "down" {
                    (at + 1).rem_euclid(len)
                } else {
                    (at - 1 + len).rem_euclid(len)
                } as usize;
                popup.focus = if in_sub {
                    MenuFocus::Sub(list[next])
                } else {
                    MenuFocus::Top(list[next])
                };
                popup.focus_visible = true;
            }
            _ => return,
        }
        window.refresh();
        cx.notify();
    }

    /// The open menu and its submenu over everything, and under them the menu
    /// that closed while it rolls up.
    pub(in crate::frontend) fn menu_overlay(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let reduce = cx.reduce_motion();
        let closed = self
            .popup_leaving
            .as_ref()
            .and_then(|(popup, at)| Some((popup, remaining(*at, reduce)?)));
        if self.popup.is_none() && closed.is_none() {
            return None;
        }
        let viewport = window.viewport_size();
        let mut animating = false;
        let mut layer = div()
            .id("menu-layer")
            .w(viewport.width)
            .h(viewport.height)
            .on_scroll(|_, _, cx| cx.stop_propagation());
        if let Some((popup, left)) = closed {
            animating = true;
            // Its surfaces are named as those of a menu opened meanwhile: kept
            // apart from them, and under them.
            layer = layer.child(
                div()
                    .id("menu-leaving")
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .children(self.menu_surfaces(popup, Some(left), &mut animating, window, cx)),
            );
        }
        if let Some(popup) = &self.popup {
            layer = layer
                // A press anywhere else closes the menu and still reaches what is there.
                .on_any_mouse_down(cx.listener(|this, _, window, cx| {
                    this.close_menu(window, cx);
                }))
                .children(self.menu_surfaces(popup, None, &mut animating, window, cx));
        }
        if animating {
            window.request_animation_frame();
        }
        Some(
            deferred(anchored().position(point(px(0.), px(0.))).child(layer))
                .with_priority(gpui_kit::base::POPUP_PRIORITY)
                .into_any_element(),
        )
    }

    /// A menu's surfaces, lowest first: the menu, the submenu that closed
    /// while it rolls up, and the open submenu. `closing` is how much is left
    /// of a closed menu: all of them roll up with it, without taking input.
    /// `animating` is set while any of them is on its way in or out.
    fn menu_surfaces(
        &self,
        popup: &Popup,
        closing: Option<f32>,
        animating: &mut bool,
        window: &Window,
        cx: &Context<Self>,
    ) -> Vec<AnyElement> {
        let reduce = cx.reduce_motion();
        let interactive = closing.is_none();
        let whole = closing.unwrap_or(1.);
        let viewport = window.viewport_size();
        let placed = |size: Size<Pixels>| {
            if reduce {
                size
            } else {
                gpui_kit::size(size.width * ENTER_SCALE, size.height * ENTER_SCALE)
            }
        };
        let layout = measure(&popup.items, window, cx);
        let origin = menu_origin(popup.position, placed(layout.size), viewport);
        // One closed while it was still fading in goes on fading in.
        let opacity = entered(popup.opened, reduce);
        *animating |= opacity < 1.;
        // A menu that opened above the pointer goes back down to it.
        let upward = origin.y < popup.position.y;
        let mut surfaces = vec![rolled(origin, layout.size, whole, upward, |origin| {
            self.menu_surface(
                popup,
                &popup.items,
                Part::Menu,
                origin,
                &layout,
                opacity,
                interactive,
                cx,
            )
        })];
        let closed = popup
            .sub_leaving
            .as_ref()
            .and_then(|(sub, at)| Some((sub, Part::Closed, remaining(*at, reduce)?)));
        let open = popup.sub.as_ref().map(|sub| (sub, Part::Submenu, 1.));
        for (sub, part, left) in closed.into_iter().chain(open) {
            let Some(owner) = popup.items.get(sub.owner) else {
                continue;
            };
            let sub_layout = measure(&owner.children, window, cx);
            let sub_origin = submenu_origin(
                origin,
                layout.size,
                layout.tops[sub.owner],
                placed(sub_layout.size),
                viewport,
            );
            let sub_opacity = entered(sub.opened, reduce);
            *animating |= part == Part::Closed || sub_opacity < 1.;
            // A submenu goes back to its first item, level with its owner.
            surfaces.push(rolled(
                sub_origin,
                sub_layout.size,
                left * whole,
                false,
                |origin| {
                    self.menu_surface(
                        popup,
                        &owner.children,
                        part,
                        origin,
                        &sub_layout,
                        sub_opacity,
                        interactive && part == Part::Submenu,
                        cx,
                    )
                },
            ));
        }
        surfaces
    }

    /// One menu (`.menu`): items and separators on the flyout surface.
    #[allow(clippy::too_many_arguments)]
    fn menu_surface(
        &self,
        popup: &Popup,
        items: &[MenuItem],
        part: Part,
        origin: Point<Pixels>,
        layout: &MenuLayout,
        opacity: f32,
        interactive: bool,
        cx: &Context<Self>,
    ) -> Stateful<Div> {
        let p = self.palette;
        let sub = part != Part::Menu;
        let entry = |index| {
            if sub {
                Entry::Sub(index)
            } else {
                Entry::Top(index)
            }
        };
        let focused = |index| match (popup.focus, sub) {
            (MenuFocus::Top(focus), false) | (MenuFocus::Sub(focus), true) => focus == index,
            _ => false,
        };
        let open_owner = popup.sub.as_ref().map(|sub| sub.owner);
        let rows = items.iter().enumerate().map(|(index, item)| {
            if item.separator {
                return div()
                    .id(("menu-separator", index))
                    .flex_shrink_0()
                    .h(px(1.))
                    .mx(px(-INSET))
                    .my(px(INSET))
                    .bg(p.stroke);
            }
            let this_entry = entry(index);
            let owner = !sub && !item.children.is_empty();
            // `:hover`, `:focus-visible` and the item whose submenu is open.
            let lit = interactive
                && (popup.hovered == Some(this_entry)
                    || (popup.focus_visible && focused(index))
                    || (owner && open_owner == Some(index)));
            // `.danger` follows `:disabled` in the stylesheet and wins over it.
            let color = if item.danger {
                p.danger
            } else if item.disabled {
                p.faint
            } else {
                p.ink
            };
            let glyph = if item.checked {
                Some("CheckIcon")
            } else {
                item.icon
            };
            div()
                .id((if sub { "submenu-item" } else { "menu-item" }, index))
                .role(Role::MenuItem)
                .aria_label(item.label.clone())
                .when(owner, |d| d.aria_expanded(open_owner == Some(index)))
                .when_some(item.hint, |d, hint| d.aria_keyshortcuts(hint))
                .flex()
                .flex_shrink_0()
                .items_center()
                .h(px(ROW))
                .px(px(GAP))
                .gap(px(GAP))
                .rounded(px(4.))
                .text_color(color)
                .when(lit, |d| d.bg(p.hover))
                .child(
                    div()
                        .flex()
                        .flex_shrink_0()
                        .size(px(ICON))
                        .when_some(glyph, |d, glyph| d.child(icon(glyph, color))),
                )
                .child(item.label.clone())
                .when_some(item.hint, |d, hint| {
                    d.child(
                        div()
                            .ml_auto()
                            .text_size(theme::font(12.))
                            .text_color(p.faint)
                            .child(hint),
                    )
                })
                .when_some(item.detail.clone(), |d, detail| {
                    d.child(
                        div()
                            .ml_auto()
                            .text_size(theme::font(12.))
                            .text_color(p.faint)
                            .child(detail),
                    )
                })
                .when(owner, |d| {
                    d.child(
                        div()
                            .when(item.detail.is_none(), |d| d.ml_auto())
                            .flex_shrink_0()
                            .child(icon("ChevronRight", p.faint).size(px(CHEVRON))),
                    )
                })
                .when(interactive, |d| {
                    d.hover_listener_mode(HoverListenerMode::InputModalityIndependent)
                        .on_hover(cx.listener(move |this, hovered: &bool, window, cx| {
                            this.menu_hover(this_entry, *hovered, window, cx);
                        }))
                })
                .when(interactive && !item.disabled, |d| {
                    d.on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        if let Some(popup) = &mut this.popup {
                            popup.focus_visible = false;
                        }
                        this.menu_activate(this_entry, window, cx);
                    }))
                })
        });
        div()
            .id(match part {
                Part::Menu => "menu",
                Part::Submenu => "submenu",
                Part::Closed => "submenu-closed",
            })
            .role(Role::Menu)
            .absolute()
            .left(origin.x)
            .top(origin.y)
            .w(layout.size.width)
            .flex()
            .flex_col()
            .p(px(INSET))
            .border_1()
            .border_color(p.stroke_strong)
            .rounded(px(8.))
            // Opaque menu faces stay readable over the page without a shadow.
            .bg(widgets::solid(p.flyout))
            .opacity(opacity)
            .text_size(theme::font(13.))
            .line_height(relative(1.4))
            .whitespace_nowrap()
            .when(!sub, |d| d.track_focus(&popup.focus_handle))
            .when(interactive, |d| {
                d.occlude()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            })
            .when(interactive && sub, |d| {
                // Reaching the submenu keeps it open.
                d.on_hover(cx.listener(|this, hovered: &bool, _, _| {
                    if *hovered && let Some(popup) = &mut this.popup {
                        popup.timer = None;
                    }
                }))
            })
            .children(rows)
    }
}
/// Task Manager's order, highest first.
const PRIORITIES: [&str; 6] = [
    "realtime",
    "high",
    "aboveNormal",
    "normal",
    "belowNormal",
    "idle",
];
impl MenuItem {
    pub(super) fn separator() -> Self {
        let mut item = Self::new(String::new(), Action::CloseOverlay);
        item.separator = true;
        item
    }
    pub(super) fn icon(mut self, icon: &'static str) -> Self {
        self.icon = Some(icon);
        self
    }
    pub(super) fn danger(mut self) -> Self {
        self.danger = true;
        self
    }
    pub(super) fn hint(mut self, hint: &'static str) -> Self {
        self.hint = Some(hint);
        self
    }
    pub(super) fn submenu(mut self, children: Vec<MenuItem>) -> Self {
        self.children = children;
        self
    }
    pub(super) fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
    /// A check toggled in place, the menu staying open.
    pub(super) fn stay(mut self) -> Self {
        self.stay = true;
        self
    }
}
impl Desktop {
    /// Opens the menu of the row at `index`, as a right press on it does. The
    /// press selects the row first, as a left one does. A heading is never
    /// selected (a left press folds it), and only a user's heading has a menu.
    pub(in crate::frontend) fn open_row_menu(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(row) = self.rows.get(index).cloned() else {
            return;
        };
        // The table outlines the row it was pressed on until told otherwise:
        // the selection shows it instead, and a heading shows nothing.
        if let Some(table) = &self.table {
            table.update(cx, |state, cx| state.set_right_clicked_row(None, cx));
        }
        if row.section.is_none() {
            self.selected.insert(self.page.clone(), row.key.clone());
        } else if row.user.is_none() {
            return;
        }
        let items = self.row_menu(&row);
        self.open_popup(items, window, cx);
        cx.notify();
    }
    pub(in crate::frontend) fn row_menu(&self, row: &Row) -> Vec<MenuItem> {
        if row.section.is_some() {
            let mut items = vec![
                MenuItem::new(
                    label(if row.expanded {
                        "common.collapse"
                    } else {
                        "common.expand"
                    }),
                    Action::Fold(row.key.clone()),
                )
                // Every match shows while searching: headings cannot fold.
                .disabled(!self.query.trim().is_empty()),
            ];
            items.extend(self.user_menu(row));
            return items;
        }
        let mut items = match self.page.as_str() {
            "processes" => self.process_menu(row),
            "services" => self.service_menu(row),
            "startup" => {
                let raw = &row.raw;
                let path = raw["path"].as_str().filter(|p| !p.is_empty());
                vec![
                    MenuItem::new(
                        label(if boolean(raw, "enabled") {
                            "common.disable"
                        } else {
                            "common.enable"
                        }),
                        Action::Invoke(
                            "set_startup_enabled".into(),
                            json!({"id":raw["id"],"enabled":!boolean(raw,"enabled")}),
                        ),
                    )
                    .disabled(!raw["blocked"].is_null()),
                    MenuItem::separator(),
                    MenuItem::new(
                        label("common.openFileLocation"),
                        Action::Invoke("reveal_in_explorer".into(), json!({"path":path})),
                    )
                    .icon("FolderIcon")
                    .disabled(!boolean(raw, "exists") || path.is_none()),
                    MenuItem::new(
                        label("startup.menu.copyCommand"),
                        Action::Copy(text(raw, "command").into(), "startup.copied.command"),
                    )
                    .icon("CopyIcon")
                    .disabled(text(raw, "command").is_empty()),
                    MenuItem::separator(),
                ]
            }
            "network" => self.connection_menu(row),
            _ => vec![],
        };
        if self.page != "processes" {
            items.extend(self.mark_menu(row));
        }
        items
    }
    fn process_menu(&self, row: &Row) -> Vec<MenuItem> {
        let group = row.members.len() > 1;
        let identity_ready = row
            .members
            .iter()
            .all(|member| number(member, "createTime") >= 0.);
        let args = json!({"members":row.members,"name":row.title});
        let mut items = Vec::new();
        if self.view == "tree" && row.expandable {
            for (expand, name, glyph) in [
                (true, "common.expand", "ChevronDown"),
                (false, "common.collapse", "ChevronRight"),
            ] {
                items.push(
                    MenuItem::new(label(name), Action::TreeExpand(row.key.clone(), expand))
                        .icon(glyph)
                        .disabled(!self.query.trim().is_empty() || row.expanded == expand),
                );
            }
            items.push(MenuItem::separator());
        }
        items.push(
            MenuItem::new(
                if group {
                    tr!("processes.menu.endAll", count = row.members.len())
                } else {
                    label("common.endProcess")
                },
                Action::Dialog("end".into(), args.clone()),
            )
            .icon("EndIcon")
            .hint("Del")
            .disabled(!identity_ready)
            .danger(),
        );
        if !group {
            items.push(
                MenuItem::new(
                    label("common.endProcessTree"),
                    Action::Dialog("endTree".into(), args.clone()),
                )
                .icon("BranchEndIcon")
                .disabled(!identity_ready),
            );
        }
        if self.data.get("desktopShell").is_some_and(|shell| {
            row.members.iter().any(|member| {
                shell["pid"] == member["pid"] && shell["createTime"] == member["createTime"]
            })
        }) {
            items.push(
                MenuItem::new(
                    label("tasks.explorer.restart"),
                    Action::Dialog("restartExplorer".into(), row.raw.clone()),
                )
                .icon("RestartIcon"),
            );
        }
        // The priority every process of the row shares, if they agree.
        let priorities = row
            .members
            .iter()
            .map(|member| priority_class(member))
            .collect::<HashSet<_>>();
        let current = (priorities.len() == 1)
            .then(|| priorities.into_iter().next().flatten())
            .flatten();
        let efficient = row
            .members
            .iter()
            .all(|member| boolean(member, "efficiency"));
        items.push(MenuItem::separator());
        items.push(
            MenuItem::new(label("processes.menu.setPriority"), Action::CloseOverlay)
                .disabled(!identity_ready)
                .icon("GaugeIcon")
                .submenu(
                    PRIORITIES
                        .into_iter()
                        .map(|priority| {
                            MenuItem::new(
                                label(&format!("processes.priority.{priority}")),
                                Action::Dialog(
                                    "priority".into(),
                                    json!({"members":row.members,"name":row.title,"priority":priority}),
                                ),
                            )
                            .checked(current == Some(priority))
                        })
                        .collect(),
                ),
        );
        // I/O and memory priority submenus, checked where every member agrees.
        for (kind, levels, glyph) in [
            ("ioPriority", &IO_PRIORITIES[..], "DiskIcon"),
            ("memoryPriority", &MEMORY_PRIORITIES[..], "MemoryIcon"),
        ] {
            let current = shared_text(&row.members, kind);
            items.push(
                MenuItem::new(label(&format!("processes.menu.{kind}")), Action::CloseOverlay)
                    .disabled(!identity_ready)
                    .icon(glyph)
                    .submenu(
                        levels
                            .iter()
                            .map(|&level| {
                                MenuItem::new(
                                    label(&format!("processes.{kind}.{level}")),
                                    Action::Dialog(
                                        kind.into(),
                                        json!({"members":row.members,"name":row.title,"priority":level}),
                                    ),
                                )
                                .checked(current == Some(level))
                            })
                            .collect(),
                    ),
            );
        }
        items.push(
            MenuItem::new(
                label("common.efficiencyMode"),
                Action::Dialog(
                    "efficiency".into(),
                    json!({"members":row.members,"name":row.title,"on":!efficient}),
                ),
            )
            .icon("LeafIcon")
            .disabled(!identity_ready)
            .checked(efficient),
        );
        // Suspending nests (each call adds one to every thread's suspend count
        // and a resume takes one off), so each action only takes the members
        // in the other state: suspending a process twice would leave it
        // suspended after one resume. A row whose app started new processes
        // after being suspended offers both. Members Windows froze (packaged
        // apps in the background) are Windows's to thaw: resuming does not,
        // so neither item takes them.
        let (suspended, running): (Vec<_>, Vec<_>) = row
            .members
            .iter()
            .filter(|member| !boolean(member, "frozen"))
            .cloned()
            .partition(|member| boolean(member, "suspended"));
        for (members, on, name, glyph) in [
            (running, true, "processes.menu.suspend", "PauseIcon"),
            (suspended, false, "processes.menu.resume", "PlayIcon"),
        ] {
            if members.is_empty() {
                continue;
            }
            items.push(
                MenuItem::new(
                    label(name),
                    Action::Dialog(
                        "suspend".into(),
                        json!({"members":members,"name":row.title,"on":on}),
                    ),
                )
                .icon(glyph)
                .disabled(!identity_ready),
            );
        }
        items.push(MenuItem::new(
            label("processes.menu.addRule"),
            Action::Dialog("ruleScope".into(), json!({"name":text(&row.raw,"name")})),
        ));
        items.push(MenuItem::separator());
        if row.expandable && self.view != "tree" {
            items.push(MenuItem::new(
                label(if row.expanded {
                    "common.collapse"
                } else {
                    "common.expand"
                }),
                Action::Fold(row.key.clone()),
            ));
        }
        let path = row.raw["path"].as_str().filter(|p| !p.is_empty());
        items.push(
            MenuItem::new(
                label("common.openFileLocation"),
                Action::Invoke("reveal_in_explorer".into(), json!({"path":path})),
            )
            .icon("FolderIcon")
            .disabled(path.is_none()),
        );
        items.push(
            MenuItem::new(
                label("common.copyPath"),
                Action::Copy(path.unwrap_or_default().into(), "actions.copyWhat.path"),
            )
            .icon("CopyIcon")
            .disabled(path.is_none()),
        );
        items.push(if group {
            MenuItem::new(
                label("common.copyName"),
                Action::Copy(text(&row.raw, "name").into(), "actions.copyWhat.name"),
            )
        } else {
            MenuItem::new(
                label("processes.menu.copyPid"),
                Action::Copy(row.raw["pid"].to_string(), "processes.menu.copyPidWhat"),
            )
        });
        items.push(MenuItem::separator());
        items.push(
            MenuItem::new(
                label("processes.menu.details"),
                Action::Preference("openDetails".into(), json!(row.key)),
            )
            .icon("InfoIcon")
            .hint("Enter"),
        );
        items
    }
    fn service_menu(&self, row: &Row) -> Vec<MenuItem> {
        let raw = &row.raw;
        let name = text(raw, "name");
        let state = text(raw, "state");
        let pending = state.ends_with("Pending");
        let control = |action: &str| {
            Action::Invoke(
                "control_service".into(),
                json!({"name":name,"action":action}),
            )
        };
        // The binary is the first token of the image path, quoted or not.
        let command = text(raw, "path").trim();
        let binary = command
            .strip_prefix('"')
            .and_then(|rest| rest.split('"').next())
            .or_else(|| command.split_whitespace().next())
            .filter(|binary| !binary.is_empty());
        let current = text(raw, "startType");
        let mut items = vec![
            MenuItem::new(label("common.start"), control("start"))
                .icon("PlayIcon")
                .disabled(state == "running" || pending),
            MenuItem::new(label("common.stop"), control("stop"))
                .icon("StopIcon")
                .disabled(!boolean(raw, "canStop") || pending),
            MenuItem::new(label("common.restart"), control("restart"))
                .icon("RestartIcon")
                .disabled(!boolean(raw, "canStop") || pending),
            MenuItem::separator(),
        ];
        for start_type in ["auto", "autoDelayed", "manual", "disabled"] {
            items.push(
                MenuItem::new(
                    // `type` is a keyword, so the placeholder is passed by name.
                    argus_core::i18n::format(
                        "services.menu.startType",
                        &[(
                            "type",
                            &label(&format!("services.startType.{start_type}"))
                                as &dyn std::fmt::Display,
                        )],
                    ),
                    Action::Invoke(
                        "set_service_start_type".into(),
                        json!({"name":name,"startType":start_type}),
                    ),
                )
                .checked(current == start_type),
            );
        }
        let pid = raw["pid"].as_u64().unwrap_or(0);
        items.extend([
            MenuItem::separator(),
            MenuItem::new(
                label("common.goToProcess"),
                Action::Preference("revealProcess".into(), json!({"pid":pid})),
            )
            .icon("GoToIcon")
            .disabled(pid == 0),
            MenuItem::new(
                label("common.openFileLocation"),
                Action::Invoke("reveal_in_explorer".into(), json!({"path":binary})),
            )
            .icon("FolderIcon")
            .disabled(binary.is_none()),
            MenuItem::new(
                label("services.menu.copyName"),
                Action::Copy(name.into(), "services.copied.name"),
            )
            .icon("CopyIcon"),
            MenuItem::separator(),
        ]);
        items
    }
    fn connection_menu(&self, row: &Row) -> Vec<MenuItem> {
        let raw = &row.raw;
        let process = values(&self.snapshot["processes"])
            .iter()
            .find(|p| p["pid"] == raw["pid"]);
        let pid = raw["pid"].as_u64().unwrap_or(0);
        let name = process
            .map(|p| text(p, "name").to_owned())
            .unwrap_or_else(|| format!("PID {pid}"));
        let v6 = boolean(raw, "ipv6");
        let endpoint = |address: &str, port: &Value| match port.as_u64() {
            Some(port) if v6 => format!("[{address}]:{port}"),
            Some(port) => format!("{address}:{port}"),
            None => address.to_owned(),
        };
        let closable = text(raw, "protocol") == "tcp" && !v6 && text(raw, "state") == "established";
        let remote = raw["remoteAddress"].as_str();
        vec![
            MenuItem::new(
                label("common.goToProcess"),
                Action::Preference("revealProcess".into(), raw.clone()),
            )
            .icon("GoToIcon")
            .disabled(process.is_none()),
            MenuItem::new(
                tr!("connections.menu.end", name = name),
                Action::Dialog(
                    "end".into(),
                    json!({"members":process.into_iter().collect::<Vec<_>>(),"name":name}),
                ),
            )
            .icon("EndIcon")
            .danger()
            .disabled(pid <= 4 || process.is_none()),
            MenuItem::new(
                label("connections.menu.close"),
                Action::Invoke("close_connection".into(), raw.clone()),
            )
            .icon("DismissIcon")
            .disabled(!closable),
            MenuItem::separator(),
            MenuItem::new(
                label("connections.menu.copyLocal"),
                Action::Copy(
                    endpoint(text(raw, "localAddress"), &raw["localPort"]),
                    "connections.copied.local",
                ),
            )
            .icon("CopyIcon"),
            MenuItem::new(
                label("connections.menu.copyRemote"),
                Action::Copy(
                    remote
                        .map(|r| endpoint(r, &raw["remotePort"]))
                        .unwrap_or_default(),
                    "connections.copied.remote",
                ),
            )
            .disabled(remote.is_none()),
            MenuItem::separator(),
        ]
    }
    /// Pin and note items, kept under the row's mark key.
    fn mark_menu(&self, row: &Row) -> Vec<MenuItem> {
        let key = row.mark_key.clone();
        let pinned = row.pinned;
        let mut items = vec![
            MenuItem::new(
                label(if pinned {
                    "common.marks.unpin"
                } else {
                    "common.marks.pin"
                }),
                Action::Preference("pin".into(), json!(key)),
            )
            .icon(if pinned { "UnpinIcon" } else { "PinIcon" })
            .disabled(key.is_none()),
            MenuItem::new(
                label(if row.note.is_some() {
                    "common.marks.editNote"
                } else {
                    "common.marks.addNote"
                }),
                Action::Dialog(
                    "note".into(),
                    json!({"key":key,"name":row.title,"note":row.note}),
                ),
            )
            .icon("NoteIcon")
            .disabled(key.is_none()),
        ];
        if let (Some(key), Some(_)) = (&row.mark_key, &row.note) {
            items.push(
                MenuItem::new(
                    label("common.marks.deleteNote"),
                    Action::Preference("deleteNote".into(), json!(key)),
                )
                .icon("TrashIcon"),
            );
        }
        items
    }
}
/// The I/O priorities a user can set; Windows keeps high and critical.
const IO_PRIORITIES: [&str; 3] = ["veryLow", "low", "normal"];
const MEMORY_PRIORITIES: [&str; 5] = ["veryLow", "low", "medium", "belowNormal", "normal"];

/// The text value of `key` every member shares; None when they differ or one lacks it.
fn shared_text<'a>(members: &'a [Value], key: &str) -> Option<&'a str> {
    let first = members.first()?[key].as_str()?;
    members
        .iter()
        .all(|member| member[key].as_str() == Some(first))
        .then_some(first)
}

/// The priority class of a base priority; other values match no class.
fn priority_class(process: &Value) -> Option<&'static str> {
    Some(match process["basePriority"].as_i64()? {
        4 => "idle",
        6 => "belowNormal",
        8 => "normal",
        10 => "aboveNormal",
        13 => "high",
        24 => "realtime",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    // Named imports keep the glob-imported GPUI `test` macro out of scope.
    use super::{Action, MenuItem, menu_origin, submenu_origin, tidy};
    use gpui_kit::{point, px, size};

    fn item(label: &str) -> MenuItem {
        MenuItem::new(label, Action::CloseOverlay)
    }

    #[test]
    fn separators_only_between_items() {
        let items = tidy(vec![
            MenuItem::separator(),
            item("a"),
            MenuItem::separator(),
            MenuItem::separator(),
            item("b").submenu(vec![MenuItem::separator(), item("c")]),
            MenuItem::separator(),
        ]);
        let shape = items
            .iter()
            .map(|item| if item.separator { "-" } else { "x" })
            .collect::<String>();
        assert_eq!(shape, "x-x");
        assert_eq!(items[2].children.len(), 1);
    }

    #[test]
    fn menu_opens_toward_the_free_side() {
        let viewport = size(px(800.), px(600.));
        let menu = size(px(220.), px(300.));
        assert_eq!(
            menu_origin(point(px(100.), px(100.)), menu, viewport),
            point(px(100.), px(100.))
        );
        // Too close to the right and bottom edges: left of and above the pointer.
        assert_eq!(
            menu_origin(point(px(700.), px(500.)), menu, viewport),
            point(px(480.), px(200.))
        );
        // Neither side has room: kept 8 px from the window's edge.
        assert_eq!(
            menu_origin(
                point(px(100.), px(200.)),
                size(px(220.), px(590.)),
                viewport
            ),
            point(px(100.), px(8.))
        );
    }

    #[test]
    fn submenu_sits_beside_its_owner() {
        let viewport = size(px(800.), px(600.));
        let menu = point(px(100.), px(100.));
        let menu_size = size(px(220.), px(300.));
        let sub = size(px(220.), px(190.));
        // 4 px over the owner's right edge, its first item level with the owner.
        assert_eq!(
            submenu_origin(menu, menu_size, 74., sub, viewport),
            point(px(311.), px(169.))
        );
        // No room on the right: on the left, overlapping the owner by 4 px.
        assert_eq!(
            submenu_origin(point(px(500.), px(100.)), menu_size, 74., sub, viewport),
            point(px(289.), px(169.))
        );
        // Near the bottom: moved up to stay 8 px inside the window.
        assert_eq!(
            submenu_origin(point(px(100.), px(400.)), menu_size, 74., sub, viewport).y,
            px(402.)
        );
    }
}
