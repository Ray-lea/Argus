//! Shared native controls and short, reduced-motion-aware transitions.
use super::*;
use gpui_kit::base::ElementExt;
use gpui_kit::component::tab::{Tab, TabBar};

/// The button faces of the original `.btn`: plain, accent, danger and ghost.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::frontend) enum Tone {
    Plain,
    Accent,
    Danger,
    Ghost,
}

/// A deferred piece of work on the desktop: a toast action, a confirmed step.
pub(in crate::frontend) type Handler = Rc<dyn Fn(&mut Desktop, &mut Window, &mut Context<Desktop>)>;
type Pick = Rc<dyn Fn(&mut Desktop, usize, &mut Window, &mut Context<Desktop>)>;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::frontend) enum ToastKind {
    Error,
    Success,
    Info,
}

/// One notice in the lower-right stack (Overlays.tsx).
pub(in crate::frontend) struct Toast {
    pub id: u64,
    pub kind: ToastKind,
    pub text: String,
    pub actions: Vec<(String, Handler)>,
    pub born: Instant,
    /// When it leaves; `None` while the pointer holds it.
    pub expires: Option<Instant>,
    pub hovered: bool,
    pub leaving: Option<Instant>,
}

/// A drop-down list that is open, and where its button is.
pub(in crate::frontend) struct OpenSelect {
    pub id: SharedString,
    pub options: Vec<String>,
    pub selected: Option<usize>,
    pub active: usize,
    pub anchor: Bounds<Pixels>,
    pub pick: Pick,
    pub opened: Instant,
}

/// The show/hide hotkey card's recording session (Settings.tsx, HotkeySetting).
#[derive(Default)]
pub(in crate::frontend) struct HotkeyCard {
    pub mode: HotkeyMode,
    pub recording: recorder::Recording,
    pub message: Option<HotkeyMessage>,
    pub candidate: String,
    /// What a save just did (turned off, or the new combination) and when.
    pub done: Option<(bool, String, Instant)>,
    pub flash: Option<Instant>,
    pub last_key: Option<Instant>,
    pub quiet_since: Option<Instant>,
    pub session: u64,
    pub switching: bool,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(in crate::frontend) enum HotkeyMode {
    #[default]
    Idle,
    Recording,
    Confirm,
    Saving,
}

#[derive(Clone)]
pub(in crate::frontend) enum HotkeyMessage {
    Error(Value),
    Issue(&'static str, String),
    Hint(recorder::Hint),
    MaybeTaken,
    Warning(Vec<&'static str>, String),
}

/// State the original kept in component signals: the settings scroller, busy
/// cards, inline inputs, the open drop-down, toasts and stacked dialogs.
#[derive(Default)]
pub(in crate::frontend) struct UiState {
    pub toasts: Vec<Toast>,
    pub toast_seq: u64,
    pub select: Option<OpenSelect>,
    pub anchors: Rc<RefCell<HashMap<SharedString, Bounds<Pixels>>>>,
    pub focus: RefCell<HashMap<SharedString, FocusHandle>>,
    /// Dialogs under the top one (`Desktop::modal`), bottom first.
    pub below: Vec<dialogs::Modal>,
    pub modal_seq: u64,
    /// Dialogs that wait for the onboarding to close.
    pub queued: Vec<(String, Value)>,
    /// Settings page: measured width, the clicked section and smooth scrolling.
    pub settings_width: Rc<Cell<Pixels>>,
    pub settings_picked: bool,
    pub settings_jump_wait: bool,
    pub settings_scroll: Option<(Pixels, Pixels, Instant)>,
    pub located: Option<(String, Instant)>,
    pub focus_rule: Option<(String, Instant)>,
    /// Cards running an action, and the errors they show.
    pub busy: HashSet<String>,
    pub errors: HashMap<String, String>,
    pub inputs: HashMap<String, Entity<InputState>>,
    pub input_subscriptions: Vec<Subscription>,
    pub proxy_draft: Option<String>,
    pub hotkey: HotkeyCard,
    pub system_motion: Option<bool>,
    pub ticks: u64,
    pub icons: RefCell<HashMap<u32, Arc<Image>>>,
    /// Measured heights of each dialog's body and footer, to centre it.
    pub dialog_parts: Rc<RefCell<HashMap<(u64, u8), Pixels>>>,
}

/// The privileged service's state in a few words (also used by the onboarding).
pub(in crate::frontend) fn service_state(status: &Value) -> String {
    if status.is_null() {
        return label("settings.privilege.state.unknown");
    }
    let state = text(status, "state");
    let key = match state {
        "notInstalled" => "notInstalled",
        "running" if boolean(status, "authorized") && status["error"].is_null() => {
            return tr!(
                "settings.privilege.state.connected",
                version = text(status, "version")
            );
        }
        "running" if boolean(status, "incompatible") => "incompatible",
        "running" => "unreachable",
        "stopped" => "stopped",
        "startPending" => "startPending",
        "stopPending" => "stopPending",
        _ => "unavailable",
    };
    label(&format!("settings.privilege.state.{key}"))
}

/// The service runs, answers and is ours.
pub(in crate::frontend) fn service_connected(status: &Value) -> bool {
    text(status, "state") == "running" && boolean(status, "authorized") && status["error"].is_null()
}

/// The colour without its transparency (surfaces drawn without blur).
pub(in crate::frontend) fn solid(color: Hsla) -> Hsla {
    Hsla { a: 1., ..color }
}

/// Lighter, like the original's `filter: brightness(1.08)` on hover.
pub(in crate::frontend) fn brighter(color: Hsla) -> Hsla {
    Hsla {
        l: (color.l * 1.08).min(1.),
        ..color
    }
}

pub(in crate::frontend) trait ButtonExt {
    /// Disables the button and dims it, as Chromium dims a disabled `.btn`.
    fn inert(self, disabled: bool) -> Self;
}
impl ButtonExt for gpui_kit::base::Button {
    fn inert(self, disabled: bool) -> Self {
        self.disabled(disabled).when(disabled, |b| b.opacity(0.5))
    }
}

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

    /// A Fluent button (`.btn`): 32 px, 1 px stroke with a stronger bottom
    /// edge, 4 px radius. Callers add `on_click` and, for disabled, `inert`.
    pub(in crate::frontend) fn btn(
        &self,
        id: impl Into<SharedString>,
        text: impl Into<SharedString>,
        glyph: Option<&str>,
        tone: Tone,
    ) -> gpui_kit::base::Button {
        self.button_face(id.into(), text.into(), glyph, tone, false)
    }

    /// The small variant (`.btn.small`): 24 px high, 8 px padding.
    pub(in crate::frontend) fn btn_small(
        &self,
        id: impl Into<SharedString>,
        text: impl Into<SharedString>,
        glyph: Option<&str>,
        tone: Tone,
    ) -> gpui_kit::base::Button {
        self.button_face(id.into(), text.into(), glyph, tone, true)
    }

    fn button_face(
        &self,
        id: SharedString,
        text: SharedString,
        glyph: Option<&str>,
        tone: Tone,
        small: bool,
    ) -> gpui_kit::base::Button {
        let p = self.palette;
        let clear = p.background.opacity(0.);
        let (bg, fg, border) = match tone {
            Tone::Plain => (p.face, p.ink, p.stroke),
            Tone::Accent => (p.accent, p.on_accent, clear),
            Tone::Danger => (p.danger, p.on_danger, clear),
            Tone::Ghost => (clear, p.ink, clear),
        };
        let icon_only = text.is_empty() && glyph.is_some();
        let height = if small { px(24.) } else { px(32.) };
        gpui_kit::base::Button::new(id)
            .relative()
            .h(height)
            .when(icon_only, |b| b.w(height))
            .when(!icon_only, |b| b.px(if small { px(8.) } else { px(12.) }))
            .flex_shrink_0()
            .gap(px(6.))
            .rounded(px(4.))
            .border_1()
            .border_color(border)
            .bg(bg)
            .text_color(fg)
            .text_size(px(13.))
            .whitespace_nowrap()
            .hover(move |s| match tone {
                Tone::Plain => s.bg(p.face_hover),
                Tone::Ghost => s.bg(p.hover),
                Tone::Accent => s.bg(brighter(p.accent)),
                Tone::Danger => s.bg(brighter(p.danger)),
            })
            .active(move |s| match tone {
                Tone::Plain => s.bg(p.face_press).text_color(p.muted),
                Tone::Ghost => s.bg(p.press).text_color(p.muted),
                Tone::Accent => s.bg(p.accent.opacity(0.9)),
                Tone::Danger => s.bg(p.danger.opacity(0.9)),
            })
            .accessibility_label(text.clone())
            .when(tone == Tone::Plain, |b| {
                // The stronger bottom edge of a Fluent control.
                b.child(
                    div()
                        .absolute()
                        .left(px(3.))
                        .right(px(3.))
                        .top(height - px(2.))
                        .h(px(1.))
                        .bg(p.stroke_strong),
                )
            })
            .when_some(glyph, |b, name| b.child(icon(name, fg)))
            .when(!text.is_empty(), |b| b.child(text))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
    }

    /// A persistent focus handle for a control drawn every frame.
    pub(in crate::frontend) fn focus_for(&self, id: &str, cx: &App) -> FocusHandle {
        self.ui
            .focus
            .borrow_mut()
            .entry(SharedString::from(id.to_owned()))
            .or_insert_with(|| cx.focus_handle())
            .clone()
    }

    /// The toggle itself: 40×20, outline only when off, accent when on; the
    /// thumb grows under the pointer and slides on Fluent's move curve.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::frontend) fn toggle(
        &self,
        id: impl Into<SharedString>,
        name: impl Into<SharedString>,
        value: bool,
        disabled: bool,
        on_change: impl Fn(&mut Desktop, bool, &mut Window, &mut Context<Desktop>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id: SharedString = id.into();
        let p = self.palette;
        let offset = gpui_kit::base::motion::transition(
            (ElementId::Name(id.clone()), "thumb"),
            if value { 20f32 } else { 0. },
            theme::fluent(167, theme::ease_move),
            window,
            cx,
        );
        let group: SharedString = format!("switch-{id}").into();
        let thumb = if value { p.on_accent } else { p.muted };
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .flex_shrink_0()
            .child(
                div()
                    .min_w(px(26.))
                    .flex()
                    .justify_end()
                    .text_color(p.muted)
                    .child(label(if value {
                        "settings.toggle.on"
                    } else {
                        "settings.toggle.off"
                    })),
            )
            .child(
                gpui_kit::base::Button::new(ElementId::Name(id.clone()))
                    .group(group.clone())
                    .role(Role::Switch)
                    .accessibility_label(name.into())
                    .relative()
                    .w(px(40.))
                    .h(px(20.))
                    .flex_shrink_0()
                    .rounded(px(10.))
                    .border_1()
                    .border_color(if value { p.accent } else { p.muted })
                    .bg(if value {
                        p.accent
                    } else {
                        p.background.opacity(0.)
                    })
                    .when(value, |b| b.hover(move |s| s.bg(brighter(p.accent))))
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .left(px(offset))
                            .w(px(18.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                div()
                                    .id("switch-thumb")
                                    .size(px(12.))
                                    .rounded_full()
                                    .bg(thumb)
                                    .group_hover(group.clone(), |s| s.size(px(14.)))
                                    .group_active(group, |s| s.w(px(17.)).h(px(14.))),
                            ),
                    )
                    .inert(disabled)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        on_change(this, !value, window, cx);
                        cx.notify();
                    }))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()),
            )
            .into_any_element()
    }

    /// Keycaps (`.keycaps`): one bordered key per part, dashed when `off`;
    /// `flash` (1 → 0) lights them in the accent after a change.
    pub(in crate::frontend) fn keycaps(
        &self,
        keys: &[String],
        off: bool,
        flash: f32,
    ) -> AnyElement {
        let p = self.palette;
        let tint = move |rest: Hsla| {
            if flash > 0. {
                rest.blend(p.accent.opacity(flash))
            } else {
                rest
            }
        };
        div()
            .flex()
            .items_center()
            .gap(px(4.))
            .children(keys.iter().map(|key| {
                div()
                    .px(px(8.))
                    .py(px(2.))
                    .border_1()
                    .border_b_2()
                    .when(off, |d| d.border_dashed())
                    .border_color(tint(p.stroke_strong))
                    .rounded(px(4.))
                    .bg(p.face)
                    .text_size(px(12.))
                    .text_color(tint(if off { p.muted } else { p.ink }))
                    .whitespace_nowrap()
                    .child(key.clone())
            }))
            .into_any_element()
    }

    /// The indeterminate ring (`.spinner`): a faint track and an accent arc.
    pub(in crate::frontend) fn spinner(&self, size: f32, color: Hsla) -> AnyElement {
        let track = color.opacity(0.22);
        div()
            .size(px(size))
            .flex_shrink_0()
            .with_animation(
                "spinner",
                Animation::new(Duration::from_millis(800)).repeat(),
                move |element, delta| {
                    element.child(
                        canvas(
                            |_, _, _| {},
                            move |bounds, _, window, _| {
                                let stroke = (size / 8.).max(1.5);
                                let center = bounds.center();
                                let radius = f32::from(bounds.size.width) / 2. - stroke / 2.;
                                let ring = |from: f32, sweep: f32| {
                                    let mut path = PathBuilder::stroke(px(stroke));
                                    let steps = 24;
                                    for step in 0..=steps {
                                        let angle = from + sweep * step as f32 / steps as f32;
                                        let at = point(
                                            center.x + px(radius * angle.cos()),
                                            center.y + px(radius * angle.sin()),
                                        );
                                        if step == 0 {
                                            path.move_to(at);
                                        } else {
                                            path.line_to(at);
                                        }
                                    }
                                    path.build().ok()
                                };
                                let tau = std::f32::consts::TAU;
                                if let Some(path) = ring(0., tau) {
                                    window.paint_path(path, track);
                                }
                                if let Some(path) = ring(delta * tau - tau / 4., tau / 4.) {
                                    window.paint_path(path, color);
                                }
                            },
                        )
                        .size_full(),
                    )
                },
            )
            .into_any_element()
    }

    /// A state pill (`.state-pill`): a coloured dot, or a spinner while busy.
    pub(in crate::frontend) fn state_pill(&self, text: String, tone: &str) -> AnyElement {
        let p = self.palette;
        div()
            .flex()
            .items_center()
            .gap(px(7.))
            .h(px(26.))
            .px(px(11.))
            .flex_shrink_0()
            .border_1()
            .border_color(p.stroke)
            .rounded(px(13.))
            .bg(p.pill)
            .text_size(px(12.))
            .whitespace_nowrap()
            .child(if tone == "busy" {
                self.spinner(10., p.accent)
            } else {
                div()
                    .size(px(8.))
                    .rounded_full()
                    .bg(match tone {
                        "ok" => p.network,
                        "warn" => p.io,
                        _ => p.faint,
                    })
                    .into_any_element()
            })
            .child(text)
            .into_any_element()
    }

    /// A drop-down (Select.tsx): a combo box button as wide as its longest
    /// choice, opening a listbox under it, or above it when there is no room.
    /// Focus stays on the button; the arrow keys move through the list.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::frontend) fn select(
        &self,
        id: impl Into<SharedString>,
        name: impl Into<SharedString>,
        options: Vec<String>,
        selected: Option<usize>,
        disabled: bool,
        pick: impl Fn(&mut Desktop, usize, &mut Window, &mut Context<Desktop>) + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id: SharedString = id.into();
        let p = self.palette;
        let open = self.ui.select.as_ref().is_some_and(|s| s.id == id);
        let focus = self.focus_for(&id, cx);
        let pick: Pick = Rc::new(pick);
        let anchors = self.ui.anchors.clone();
        let anchor_id = id.clone();
        let toggle: Handler = {
            let id = id.clone();
            let options = options.clone();
            Rc::new(move |this, window, cx| {
                if this.ui.select.as_ref().is_some_and(|s| s.id == id) {
                    this.ui.select = None;
                } else {
                    let anchor = this
                        .ui
                        .anchors
                        .borrow()
                        .get(&id)
                        .copied()
                        .unwrap_or_default();
                    this.ui.select = Some(OpenSelect {
                        id: id.clone(),
                        options: options.clone(),
                        selected,
                        active: selected.unwrap_or(0),
                        anchor,
                        pick: pick.clone(),
                        opened: Instant::now(),
                    });
                }
                window.refresh();
                cx.notify();
            })
        };
        let is_open = {
            let id = id.clone();
            move |this: &Desktop| this.ui.select.as_ref().is_some_and(|s| s.id == id)
        };
        let step =
            |this: &mut Desktop, delta: isize, window: &mut Window, cx: &mut Context<Desktop>| {
                if let Some(open) = &mut this.ui.select {
                    let last = open.options.len().saturating_sub(1) as isize;
                    open.active = (open.active as isize + delta).clamp(0, last) as usize;
                    window.refresh();
                    cx.notify();
                }
            };
        gpui_kit::base::Button::new(ElementId::Name(id.clone()))
            .track_focus(&focus)
            .role(Role::ComboBox)
            .accessibility_label(name.into())
            .key_context("Combobox")
            .relative()
            .h(px(32.))
            .pl(px(11.))
            .pr(px(10.))
            .gap(px(10.))
            .flex_shrink_0()
            .justify_start()
            .rounded(px(4.))
            .border_1()
            .border_color(p.stroke)
            .bg(if open { p.face_hover } else { p.face })
            .text_color(p.ink)
            .text_size(px(13.))
            .hover(move |s| s.bg(p.face_hover))
            .child(
                div()
                    .absolute()
                    .left(px(3.))
                    .right(px(3.))
                    .top(px(30.))
                    .h(px(1.))
                    .bg(p.stroke_strong),
            )
            .child(
                // Every label in one column, the others without height: the
                // button is as wide as the longest.
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .children(options.iter().enumerate().map(|(index, option)| {
                        div()
                            .whitespace_nowrap()
                            .when(Some(index) != selected, |d| {
                                d.h_0().overflow_hidden().invisible()
                            })
                            .child(option.clone())
                    })),
            )
            .child(icon("ChevronDown", p.muted).size(px(12.)))
            .on_prepaint(move |bounds, _, _| {
                anchors.borrow_mut().insert(anchor_id, bounds);
            })
            .inert(disabled)
            .on_click({
                let toggle = toggle.clone();
                cx.listener(move |this, _, window, cx| toggle(this, window, cx))
            })
            .on_action({
                let toggle = toggle.clone();
                let is_open = is_open.clone();
                cx.listener(
                    move |this, _: &gpui_kit::base::actions::SelectUp, window, cx| {
                        if is_open(this) {
                            step(this, -1, window, cx);
                        } else {
                            toggle(this, window, cx);
                        }
                    },
                )
            })
            .on_action({
                let toggle = toggle.clone();
                let is_open = is_open.clone();
                cx.listener(
                    move |this, _: &gpui_kit::base::actions::SelectDown, window, cx| {
                        if is_open(this) {
                            step(this, 1, window, cx);
                        } else {
                            toggle(this, window, cx);
                        }
                    },
                )
            })
            .on_action({
                let toggle = toggle.clone();
                let is_open = is_open.clone();
                cx.listener(
                    move |this, _: &gpui_kit::base::actions::Confirm, window, cx| {
                        if is_open(this) {
                            this.pick_select(None, window, cx);
                        } else {
                            toggle(this, window, cx);
                        }
                    },
                )
            })
            .on_action({
                let is_open = is_open.clone();
                cx.listener(
                    move |this, _: &gpui_kit::base::actions::Cancel, window, cx| {
                        if is_open(this) {
                            // Only the list closes, not a dialog around it.
                            this.ui.select = None;
                            window.refresh();
                            cx.notify();
                        } else {
                            cx.propagate();
                        }
                    },
                )
            })
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                if !is_open(this) {
                    return;
                }
                let Some(open) = &mut this.ui.select else {
                    return;
                };
                match event.keystroke.key.as_str() {
                    "home" => open.active = 0,
                    "end" => open.active = open.options.len().saturating_sub(1),
                    "tab" => this.ui.select = None,
                    _ => return,
                }
                window.refresh();
                cx.notify();
                cx.stop_propagation();
            }))
            .into_any_element()
    }

    /// Takes the active (or given) option of the open drop-down.
    pub(in crate::frontend) fn pick_select(
        &mut self,
        index: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(open) = self.ui.select.take() else {
            return;
        };
        let index = index.unwrap_or(open.active);
        let focus = self.ui.focus.borrow().get(&open.id).cloned();
        if let Some(focus) = focus {
            focus.focus(window, cx);
        }
        if Some(index) != open.selected && index < open.options.len() {
            (open.pick)(self, index, window, cx);
        }
        window.refresh();
        cx.notify();
    }

    /// The open drop-down's listbox, over everything including dialogs.
    pub(in crate::frontend) fn select_overlay(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        const ITEM: f32 = 32.;
        const GAP: f32 = 4.;
        const MARGIN: f32 = 8.;
        let open = self.ui.select.as_ref()?;
        let p = self.palette;
        let viewport = window.viewport_size();
        let anchor = open.anchor;
        let below = f32::from(viewport.height - anchor.bottom()) - GAP - MARGIN;
        let above = f32::from(anchor.top()) - GAP - MARGIN;
        let wanted = open.options.len() as f32 * ITEM + 10.;
        let up = below < wanted && above > below;
        let max_height = (if up { above } else { below }).max(ITEM * 3.);
        let height = wanted.min(max_height);
        let top = if up {
            anchor.top() - px(GAP + height)
        } else {
            anchor.bottom() + px(GAP)
        };
        let left = anchor
            .left()
            .min(viewport.width - px(MARGIN) - anchor.size.width)
            .max(px(MARGIN));
        let opacity = if cx.reduce_motion() {
            1.
        } else {
            (open.opened.elapsed().as_secs_f32() / 0.083).min(1.)
        };
        if opacity < 1. {
            window.request_animation_frame();
        }
        let list = div()
            .id("select-list")
            .occlude()
            .absolute()
            .left(left)
            .top(top)
            .min_w(anchor.size.width)
            .max_h(px(max_height))
            .overflow_y_scroll()
            .p(px(4.))
            .border_1()
            .border_color(p.stroke_strong)
            .rounded(px(8.))
            .bg(p.flyout)
            .shadow(flyout_shadow(p.dark))
            .opacity(opacity)
            .text_size(px(13.))
            .text_color(p.ink)
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .children(open.options.iter().enumerate().map(|(index, option)| {
                let selected = Some(index) == open.selected;
                div()
                    .id(("select-option", index))
                    .relative()
                    .h(px(ITEM))
                    .px(px(12.))
                    .flex()
                    .items_center()
                    .rounded(px(4.))
                    .whitespace_nowrap()
                    .when(index == open.active, |d| d.bg(p.hover))
                    .active(move |s| s.bg(p.press))
                    .when(selected, |d| {
                        d.child(
                            div()
                                .absolute()
                                .left_0()
                                .top(px((ITEM - 16.) / 2.))
                                .w(px(3.))
                                .h(px(16.))
                                .rounded(px(2.))
                                .bg(p.accent),
                        )
                    })
                    .child(option.clone())
                    .on_mouse_move(cx.listener(move |this, _: &MouseMoveEvent, window, cx| {
                        if let Some(open) = &mut this.ui.select
                            && open.active != index
                        {
                            open.active = index;
                            window.refresh();
                            cx.notify();
                        }
                    }))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.pick_select(Some(index), window, cx);
                    }))
            }));
        Some(
            deferred(
                anchored().position(point(px(0.), px(0.))).child(
                    div()
                        .id("select-scrim")
                        .w(viewport.width)
                        .h(viewport.height)
                        .occlude()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, window, cx| {
                                this.ui.select = None;
                                window.refresh();
                                cx.notify();
                            }),
                        )
                        .on_scroll_wheel(cx.listener(|this, _, window, cx| {
                            this.ui.select = None;
                            window.refresh();
                            cx.notify();
                        }))
                        .child(list),
                ),
            )
            .with_priority(gpui_kit::base::POPUP_PRIORITY + 2)
            .into_any_element(),
        )
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

/// The flyout shadow of menus, lists and dialogs (`--shadow-flyout`).
pub(in crate::frontend) fn flyout_shadow(dark: bool) -> Vec<BoxShadow> {
    vec![
        BoxShadow {
            color: hsla(0., 0., 0., if dark { 0.42 } else { 0.14 }),
            offset: point(px(0.), px(if dark { 12. } else { 8. })),
            blur_radius: px(if dark { 32. } else { 28. }),
            spread_radius: px(0.),
            inset: false,
        },
        BoxShadow {
            color: hsla(0., 0., 0., if dark { 0.25 } else { 0.06 }),
            offset: point(px(0.), px(2.)),
            blur_radius: px(if dark { 8. } else { 6. }),
            spread_radius: px(0.),
            inset: false,
        },
    ]
}
