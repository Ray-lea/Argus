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
/// `flyout-in` and the leaving `fade-out`.
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
            position: window.mouse_position(),
            viewport: window.viewport_size(),
            opened: Instant::now(),
            focus_handle,
            focus_visible: false,
            hovered: None,
            sub: None,
            timer: None,
        });
        window.refresh();
        cx.notify();
    }

    /// Closes the menu, which fades out. The keyboard goes back to the window
    /// unless an item gave it to something else.
    pub(in crate::frontend) fn close_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mut popup) = self.popup.take() else {
            return;
        };
        popup.timer = None;
        if popup.focus_handle.contains_focused(window, cx) {
            self.focus.focus(window, cx);
        }
        self.popup_leaving = Some((popup, Instant::now()));
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
            popup.sub = Some(Submenu {
                owner,
                opened: Instant::now(),
            });
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
        let owner = popup.sub.take().map(|sub| sub.owner);
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
        let Some((owner, action)) = popup
            .item(entry)
            .filter(|item| !item.disabled && !item.separator)
            .map(|item| (!item.children.is_empty(), item.action.clone()))
        else {
            return;
        };
        popup.timer = None;
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

    /// The open menu and its submenu, or the one fading out, over everything.
    pub(in crate::frontend) fn menu_overlay(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let reduce = cx.reduce_motion();
        let fade_in = |since: Instant| {
            if reduce {
                1.
            } else {
                theme::ease_out((since.elapsed().as_secs_f32() / ENTER_SECONDS).min(1.))
            }
        };
        let leaving = self
            .popup_leaving
            .as_ref()
            .filter(|(_, at)| !reduce && at.elapsed().as_secs_f32() < LEAVE_SECONDS);
        let (popup, opacity, interactive) = match (&self.popup, leaving) {
            (Some(popup), _) => (popup, fade_in(popup.opened), true),
            // Fades out linearly, without taking input; its submenu goes at once.
            (None, Some((popup, at))) => (
                popup,
                1. - at.elapsed().as_secs_f32() / LEAVE_SECONDS,
                false,
            ),
            (None, None) => return None,
        };
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
        let mut animating = opacity < 1.;
        let mut layer = div()
            .id("menu-layer")
            .w(viewport.width)
            .h(viewport.height)
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .child(self.menu_surface(
                popup,
                &popup.items,
                false,
                origin,
                &layout,
                opacity,
                interactive,
                cx,
            ));
        if interactive {
            // A press anywhere else closes the menu and still reaches what is there.
            layer = layer.on_any_mouse_down(cx.listener(|this, _, window, cx| {
                this.close_menu(window, cx);
            }));
            if let Some(sub) = &popup.sub {
                let children = popup.children();
                let sub_layout = measure(children, window, cx);
                let sub_origin = submenu_origin(
                    origin,
                    layout.size,
                    layout.tops[sub.owner],
                    placed(sub_layout.size),
                    viewport,
                );
                let sub_opacity = fade_in(sub.opened);
                animating |= sub_opacity < 1.;
                layer = layer.child(self.menu_surface(
                    popup,
                    children,
                    true,
                    sub_origin,
                    &sub_layout,
                    sub_opacity,
                    true,
                    cx,
                ));
            }
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

    /// One menu (`.menu`): items and separators on the flyout surface.
    #[allow(clippy::too_many_arguments)]
    fn menu_surface(
        &self,
        popup: &Popup,
        items: &[MenuItem],
        sub: bool,
        origin: Point<Pixels>,
        layout: &MenuLayout,
        opacity: f32,
        interactive: bool,
        cx: &Context<Self>,
    ) -> Stateful<Div> {
        let p = self.palette;
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
                .when(owner, |d| {
                    d.child(
                        div()
                            .ml_auto()
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
            .id(if sub { "submenu" } else { "menu" })
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
            // No shadow, as the interface's menus (window.md); opaque, where the
            // original blurs what is behind its translucent face.
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
}
impl Desktop {
    pub(in crate::frontend) fn row_menu(&self, row: &Row) -> Vec<MenuItem> {
        if row.section.is_some() {
            return vec![MenuItem::new(
                label(if row.expanded {
                    "common.collapse"
                } else {
                    "common.expand"
                }),
                Action::Fold(row.key.clone()),
            )];
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
        let args = json!({"members":row.members,"name":row.title});
        let mut items = vec![
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
            .danger(),
        ];
        if !group {
            items.push(
                MenuItem::new(
                    label("common.endProcessTree"),
                    Action::Dialog("endTree".into(), args.clone()),
                )
                .icon("BranchEndIcon"),
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
        items.push(
            MenuItem::new(
                label("common.efficiencyMode"),
                Action::Dialog(
                    "efficiency".into(),
                    json!({"members":row.members,"name":row.title,"on":!efficient}),
                ),
            )
            .icon("LeafIcon")
            .checked(efficient),
        );
        items.push(MenuItem::new(
            label("processes.menu.addRule"),
            Action::Dialog("ruleScope".into(), json!({"name":text(&row.raw,"name")})),
        ));
        items.push(MenuItem::separator());
        if row.expandable {
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
