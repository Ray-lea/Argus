//! Shared native controls and short, reduced-motion-aware transitions.
use super::*;
use gpui_kit::base::ElementExt;

/// Shared button faces: plain, accent, danger and ghost.
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

/// One notice in the lower-right stack.
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
#[derive(Clone)]
pub(in crate::frontend) struct OpenSelect {
    pub id: SharedString,
    pub options: Vec<String>,
    pub selected: Option<usize>,
    pub active: usize,
    pub anchor: Bounds<Pixels>,
    pub pick: Pick,
    pub opened: Instant,
    pub scroll: ScrollHandle,
}

/// The show/hide hotkey card's recording session.
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

/// Shared control state: the settings scroller, busy
/// cards, inline inputs, the open drop-down, toasts and stacked dialogs.
#[derive(Default)]
pub(in crate::frontend) struct UiState {
    pub toasts: Vec<Toast>,
    pub toast_seq: u64,
    pub select: Option<OpenSelect>,
    /// The drop-down list that just closed, fading out (`.menu.leaving`).
    pub select_leaving: Option<(OpenSelect, Instant)>,
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
    /// File-lock searches started: each result fades in once and its list
    /// starts at the top.
    pub lock_searches: u64,
    /// The start's administrator fallback was told, or there was none.
    pub administrator_announced: bool,
    pub inputs: HashMap<String, Entity<InputState>>,
    pub input_subscriptions: Vec<Subscription>,
    pub proxy_draft: Option<String>,
    /// A number box's spin button while it is held.
    pub spin: Option<Spin>,
    /// The spin button half under the pointer: its setting and whether it is the up half.
    pub spin_hover: Option<(&'static str, bool)>,
    pub hotkey: HotkeyCard,
    pub system_motion: Option<bool>,
    pub ticks: u64,
    pub icons: RefCell<HashMap<u32, Arc<Image>>>,
    /// Measured heights of each dialog's body and footer, to size and place it.
    pub dialog_parts: Rc<RefCell<HashMap<(u64, u8), Pixels>>>,
    /// Where each shown dialog sits and the height it is drawn at.
    pub dialog_frames: RefCell<HashMap<u64, dialogs::DialogFrame>>,
    /// Segmented controls: where each one's selection was and is (x, width).
    pub slides: Rc<RefCell<HashMap<SharedString, Slide>>>,
    /// When each dialog was first drawn visible: its entrance runs from there,
    /// not from a first frame that may take long to build.
    pub dialog_shown: RefCell<HashMap<u64, Instant>>,
    /// The top dialog fading out before it closes: its id, when it started and
    /// whether it was cancelled (Escape or a press outside it).
    pub dialog_leaving: Option<(u64, Instant, bool)>,
    /// The narrowest search box: placeholder, text size and width (`Desktop::search_fit`).
    pub search_fit: RefCell<Option<(String, Pixels, Pixels)>>,
    /// How compact the toolbar is (`Desktop::fit_toolbar`).
    pub toolbar_fit: RefCell<ToolbarFit>,
    /// What the page navigation's selection mark was last placed for.
    pub nav_mark: Cell<NavMark>,
}

/// The page navigation's selection mark glides to a newly chosen page; when
/// the entries move for any other reason (the other position, the pane
/// opening, a reflow) it is placed there at once, like the interface's
/// Indicator. A new `epoch` starts a transition that adopts its first value.
#[derive(Clone, Copy, Default)]
pub(in crate::frontend) struct NavMark {
    pub top: bool,
    pub selected: usize,
    /// The entries' container as measured.
    pub size: Size<Pixels>,
    pub epoch: u64,
}

/// A number box's spin button held down (`::-webkit-inner-spin-button`).
pub(in crate::frontend) struct Spin {
    /// The setting whose box it steps.
    pub setting: &'static appearance::Setting,
    /// The half under the pointer (up is `true`), `None` off the button:
    /// while held it steps that way, as Chromium's does.
    pub over: Option<bool>,
    /// Steps again while it stays held.
    pub repeat: Task<()>,
}

/// The toolbar's compact level and what it learned (the interface's
/// `fitToolbar`): 1 hides the text of the buttons at the end, 2 the labels of
/// the view modes, 3 moves the page's controls to a row of their own and 4
/// the search box too. `need[level]` is the width the toolbar wanted when that
/// level stopped fitting; it returns to the level once the window is as wide.
#[derive(Default)]
pub(in crate::frontend) struct ToolbarFit {
    /// What the levels were learned for (page, text, navigation, language,
    /// update button); a change starts over from level 0.
    pub key: String,
    pub level: u8,
    pub need: [Pixels; 4],
    /// The level the toolbar was last drawn at: its regions measure that one.
    pub drawn: Option<u8>,
}

/// The sliding selection of one segmented control.
#[derive(Clone)]
pub(in crate::frontend) struct Slide {
    pub selected: usize,
    /// The transform it is drawn gliding with, until the glide ends.
    pub live: Option<LiveTransform>,
    pub to: Option<(f32, f32)>,
    /// The page visit it was last drawn in.
    pub epoch: u64,
}

/// Records an element's bounds; a change draws the view it is in (its region)
/// again once, so geometry that depends on layout (a sliding selection)
/// catches up on the next frame. Notifying the desktop would draw every region
/// and table again: a page's controls move on its first frame, in the middle
/// of its entrance.
fn remember(
    key: SharedString,
    anchors: Rc<RefCell<HashMap<SharedString, Bounds<Pixels>>>>,
) -> impl Fn(Bounds<Pixels>, &mut Window, &mut App) + 'static {
    move |bounds, window, cx| {
        if anchors.borrow().get(&key) == Some(&bounds) {
            return;
        }
        anchors.borrow_mut().insert(key.clone(), bounds);
        let view = window.current_view();
        window.defer(cx, move |_, cx| cx.notify(view));
    }
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

/// The stronger bottom edge of a Fluent control, over its 1 px stroke; the
/// control must be `relative`.
pub(in crate::frontend) fn control_edge(p: &Palette) -> Div {
    div()
        .absolute()
        .left(px(3.))
        .right(px(3.))
        .bottom(px(-1.))
        .h(px(1.))
        .bg(p.control_edge)
}

/// The accent bottom of a Fluent text box (`border-bottom: <width> solid
/// var(--accent)`, 2 px while focused): over the box's 1 px stroke and the
/// rest inside it. A box paints its border over its children, so this goes on
/// a `relative` wrapper without a border, laid over the box.
pub(in crate::frontend) fn accent_edge(color: Hsla, width: Pixels) -> Div {
    div()
        .absolute()
        .left(px(3.))
        .right(px(3.))
        .bottom_0()
        .h(width)
        .bg(color)
}

/// The ring of a focused box (`:focus-visible`: a 2 px outline 1 px outside
/// its edge, around its 4 px corners). `border` is the box's own border
/// width, which absolutely placed children start inside of. The box must be
/// `relative`.
pub(in crate::frontend) fn focus_ring(color: Hsla, border: Pixels) -> Div {
    let out = -(border + px(3.));
    div()
        .absolute()
        .top(out)
        .bottom(out)
        .left(out)
        .right(out)
        .border_2()
        .border_color(color)
        .rounded(px(7.))
}

/// How a single-line text box draws itself.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::frontend) enum Field {
    /// `.input`: 32 px, the control face in a 1 px stroke with a stronger
    /// bottom edge; focused, the bottom becomes a 2 px accent line.
    Boxed,
    /// The run dialog's command box (`.new-task-input > input`): 36 px with no
    /// face of its own; focused, the 2 px accent ring of `:focus-visible`.
    Bare,
}

/// A text box's placeholder (`::placeholder`): `--ink-3` in the interface
/// face at the 13 px base size, whatever the box's own font. The library's
/// placeholder takes the input's font and `--ink-2`, so boxes leave theirs
/// empty and draw this one under the text, where the caret stays visible. It
/// takes the input's line height, so it sits on the line the text will.
/// `left` and `right` are the box's padding, where its text starts and ends.
pub(in crate::frontend) fn placeholder(
    text: SharedString,
    left: Pixels,
    right: Pixels,
    p: &Palette,
) -> Div {
    div()
        .absolute()
        .top_0()
        .bottom_0()
        .left(left)
        .right(right)
        .flex()
        .items_center()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_size(theme::font(13.))
        .line_height(rems(1.25))
        .text_color(p.faint)
        .child(text)
}

/// Tooltip text size: the system's tooltip font, 9 pt at 96 dpi.
/// It follows the font size setting
/// like the rest of the interface's text.
const TOOLTIP_SIZE: f32 = 12.;
/// Tooltip lines are as tall as the tooltip font's own line spacing (Segoe
/// UI's ascent and descent, 1.33 em), as the system draws tooltip text.
const TOOLTIP_LINE_HEIGHT: f32 = 4. / 3.;

/// A hover hint with one line of text.
pub(in crate::frontend) fn tooltip(
    text: impl Into<SharedString>,
    window: &mut Window,
    cx: &mut App,
) -> AnyView {
    tooltip_text(gpui_kit::component::tooltip::Tooltip::new(text.into()), cx).build(window, cx)
}

/// A tooltip set in the system tooltip font. Tooltips are laid out apart
/// from the window's root, so nothing of its text style reaches them: without
/// the fallback families, an English system locale draws Chinese in the
/// Japanese Yu Gothic UI.
fn tooltip_text(
    tooltip: gpui_kit::component::tooltip::Tooltip,
    cx: &App,
) -> gpui_kit::component::tooltip::Tooltip {
    let (family, fallbacks) = theme::tooltip_font(cx);
    let mut tooltip = tooltip
        .font_family(family)
        .text_size(theme::font(TOOLTIP_SIZE))
        .line_height(relative(TOOLTIP_LINE_HEIGHT));
    tooltip.text_style().font_fallbacks = Some(fallbacks);
    tooltip
}

/// A tooltip for a sentence or two: it wraps at `max` like a system tooltip
/// (the interface's `title`), where a plain one runs on in a single line, past
/// the window. Tooltips are laid out at their min-content size, which for text
/// is its unwrapped width, so a long text gets the width it wraps at.
pub(in crate::frontend) fn wrapping_tooltip(
    text: SharedString,
    max: Pixels,
    window: &mut Window,
    cx: &mut App,
) -> AnyView {
    let (family, fallbacks) = theme::tooltip_font(cx);
    let mut tooltip_font = font(family);
    tooltip_font.fallbacks = Some(fallbacks);
    let size = theme::font(TOOLTIP_SIZE);
    let natural = text
        .split('\n')
        .map(|line| {
            let run = TextRun {
                len: line.len(),
                font: tooltip_font.clone(),
                color: Hsla::default(),
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            window
                .text_system()
                .shape_line(SharedString::from(line.to_owned()), size, &[run], None)
                .width
        })
        .fold(px(0.), Pixels::max);
    tooltip_text(
        gpui_kit::component::tooltip::Tooltip::element(move |_, _| {
            div().when(natural > max, |d| d.w(max)).child(text.clone())
        }),
        cx,
    )
    .build(window, cx)
}

/// Lighter, like the interface's `filter: brightness(1.08)` on hover.
pub(in crate::frontend) fn brighter(color: Hsla) -> Hsla {
    Hsla {
        l: (color.l * 1.08).min(1.),
        ..color
    }
}

pub(in crate::frontend) trait ButtonExt {
    /// Disables the button; it looks as before, as the interface's disabled
    /// `.btn` (its styles do not change for `:disabled`).
    fn inert(self, disabled: bool) -> Self;
    /// Disables the button and dims it to half, as it dims a
    /// disabled `.select` and the hotkey controls while a key is recorded.
    fn inert_dimmed(self, disabled: bool) -> Self;
}
impl ButtonExt for gpui_kit::base::Button {
    fn inert(self, disabled: bool) -> Self {
        self.disabled(disabled)
    }
    fn inert_dimmed(self, disabled: bool) -> Self {
        self.disabled(disabled).when(disabled, |b| b.opacity(0.5))
    }
}

/// How long a segmented control's selection takes to glide to another choice.
const SLIDE: Duration = Duration::from_millis(250);

impl Desktop {
    /// A segmented control (`.segmented`): a layer-coloured track with a
    /// 1 px stroke, whose raised selection (`.seg-thumb`) slides to the chosen
    /// option. Like the interface's Indicator it glides only when the choice
    /// changes in view; shown again (another page visit) it is already there.
    pub(in crate::frontend) fn segments(
        &self,
        id: impl Into<SharedString>,
        choices: Vec<(String, Option<&str>, Action, bool)>,
        cx: &Context<Self>,
    ) -> AnyElement {
        let id: SharedString = id.into();
        let p = self.palette;
        let selected = choices.iter().position(|choice| choice.3).unwrap_or(0);
        let anchors = self.ui.anchors.clone();
        let item_key = |index: usize| SharedString::from(format!("seg:{id}:{index}"));
        let track_key = SharedString::from(format!("seg:{id}"));
        let geometry = {
            let anchors = anchors.borrow();
            match (anchors.get(&track_key), anchors.get(&item_key(selected))) {
                (Some(track), Some(item)) => Some((
                    f32::from(item.left() - track.left()),
                    f32::from(item.size.width),
                )),
                _ => None,
            }
        };
        let (to, live) = {
            let mut slides = self.ui.slides.borrow_mut();
            let epoch = self.page_epoch;
            let glide = SharedString::from(format!("seg-slide:{id}"));
            let slide = slides.entry(id.clone()).or_insert(Slide {
                selected,
                live: None,
                to: geometry,
                epoch,
            });
            if slide.epoch != epoch {
                slide.epoch = epoch;
                slide.live = None;
                slide.selected = selected;
                self.live_motions.stop(&glide);
            } else if slide.selected != selected {
                slide.selected = selected;
                slide.live = None;
                // The interface's Indicator: --t-slow on the move curve. Laid out
                // at the new choice and drawn gliding there with a live
                // transform (moved, and stretched without thickening its ring
                // or changing its corners), so that the view around it replays
                // its paint instead of rendering on every frame of the glide.
                if let (Some((from_x, from_width)), Some((x, width))) = (slide.to, geometry)
                    && (from_x, from_width) != (x, width)
                    && !cx.reduce_motion()
                {
                    let transform = LiveTransform::default();
                    let stepped = transform.clone();
                    let slides = self.ui.slides.clone();
                    let key = id.clone();
                    self.live_motions.start(
                        glide,
                        SLIDE,
                        move |elapsed| {
                            let (left, wide) = if elapsed >= SLIDE {
                                (x, width)
                            } else {
                                let t =
                                    theme::ease_move(elapsed.as_secs_f32() / SLIDE.as_secs_f32());
                                (
                                    from_x + (x - from_x) * t,
                                    from_width + (width - from_width) * t,
                                )
                            };
                            // The pill is the option's box plus its 1 px ring.
                            stepped.set(LiveTransformValue {
                                offset: point(px(left - x), px(0.)),
                                scale_x: (wide + 2.) / (width + 2.),
                                ..LiveTransformValue::default()
                            });
                        },
                        move |_| {
                            if let Some(slide) = slides.borrow_mut().get_mut(&key) {
                                slide.live = None;
                            }
                        },
                    );
                    slide.live = Some(transform);
                }
            }
            if geometry.is_some() {
                slide.to = geometry;
            }
            (slide.to, slide.live.clone())
        };
        let mut track = div()
            .id(ElementId::Name(id.clone()))
            .relative()
            .flex()
            .items_center()
            .gap(px(2.))
            .p(px(2.))
            .flex_shrink_0()
            .border_1()
            .border_color(p.stroke)
            .rounded(px(6.))
            .bg(p.layer)
            // The track's padding box, the frame the sliding selection is placed
            // in: `on_prepaint` would measure from where its probe happens to sit.
            .child({
                let remember = remember(track_key, anchors.clone());
                canvas(
                    move |bounds, window, cx| remember(bounds, window, cx),
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0()
            });
        if let Some((x, width)) = to {
            // The option's box plus the interface's 1 px `--stroke` ring around
            // it. The ring is a border of its own, with the face inside it: GPUI
            // paints a shadow, and a background, under the whole box, where they
            // would show through the translucent dark-theme face and ring.
            // Positions are inside the track's stroke, as the track is measured.
            let x = x - 1.;
            let pill = div()
                .absolute()
                .top(px(1.))
                .bottom(px(1.))
                .left(px(x))
                .w(px(width + 2.))
                .rounded(px(5.))
                .border_1()
                .border_color(p.stroke)
                .child(div().size_full().rounded(px(4.)).bg(p.control_checked))
                .when(p.control_checked.a >= 1., |pill| {
                    pill.shadow(vec![BoxShadow {
                        color: hsla(0., 0., 0., 0.08),
                        offset: point(px(0.), px(1.)),
                        blur_radius: px(2.),
                        spread_radius: px(0.),
                        inset: false,
                    }])
                });
            track = track.child(match live {
                Some(live) => super::shift::live(
                    pill,
                    live,
                    super::shift::Anchor::TopLeft,
                    Rc::new(Cell::new(Point::default())),
                )
                .into_any_element(),
                None => pill.into_any_element(),
            });
        }
        for (index, (name, glyph, action, _)) in choices.into_iter().enumerate() {
            let chosen = index == selected;
            let icon_only = name.is_empty();
            let accessible = if icon_only && let Action::View(mode) = &action {
                label(&format!("shell.viewMode.{mode}.label"))
            } else {
                name.clone()
            };
            let color = if chosen { p.ink } else { p.muted };
            let hover = p.hover;
            let ink = p.ink;
            track = track.child(
                gpui_kit::base::Button::new(ElementId::NamedInteger(
                    format!("seg-{id}").into(),
                    index as u64,
                ))
                .relative()
                .h(px(26.))
                .px(px(10.))
                .flex()
                .items_center()
                .justify_center()
                .gap(px(6.))
                .rounded(px(4.))
                .text_size(theme::font(13.))
                .whitespace_nowrap()
                .text_color(color)
                // Before the first layout there is no pill yet: paint the choice itself.
                .when(chosen && to.is_none(), |b| b.bg(p.control_checked))
                .when(!chosen, |b| b.hover(move |s| s.bg(hover).text_color(ink)))
                .role(Role::Tab)
                .accessibility_label(accessible)
                .when_some(glyph, |b, glyph| b.child(icon(glyph, color)))
                .when(!icon_only, |b| b.child(name))
                .on_prepaint(remember(item_key(index), anchors.clone()))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.act(action.clone(), window, cx);
                }))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()),
            );
        }
        track.into_any_element()
    }

    /// A single-line text box drawn as the interface's (`field`), around
    /// `input` (built by the caller from `state`, with its own font or
    /// read-only state). `enabled` is false while the box is disabled: the
    /// original drew it unchanged, without its focus ring.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::frontend) fn text_field(
        &self,
        state: &Entity<InputState>,
        input: Input,
        field: Field,
        placeholder_text: SharedString,
        enabled: bool,
        window: &Window,
        cx: &App,
    ) -> Div {
        let p = self.palette;
        let state = state.read(cx);
        let focused = enabled && state.focus_handle(cx).is_focused(window);
        let empty = state.value().is_empty();
        let (height, pad) = match field {
            Field::Boxed => (px(32.), px(10.)),
            Field::Bare => (px(36.), px(11.)),
        };
        let face = div()
            .relative()
            .size_full()
            .px(pad)
            .flex()
            .items_center()
            .when(field == Field::Boxed, |d| {
                d.border_1()
                    .border_color(p.stroke)
                    .rounded(px(4.))
                    .bg(p.control_face)
                    .when(!focused, |d| d.child(control_edge(&p)))
            })
            .when(empty, |d| {
                d.child(placeholder(placeholder_text.clone(), pad, pad, &p))
            })
            .child(
                input
                    .appearance(false)
                    .px_0()
                    .py_0()
                    .text_size(theme::font(13.))
                    .aria_label(placeholder_text),
            );
        div().relative().h(height).child(face).when(focused, |d| {
            d.child(match field {
                Field::Boxed => accent_edge(p.accent, px(2.)),
                Field::Bare => focus_ring(p.accent, px(0.)),
            })
        })
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
        self.button_face(id.into(), text.into(), glyph, tone, false, None)
    }

    /// The small variant (`.btn.small`): 24 px high, 8 px padding.
    pub(in crate::frontend) fn btn_small(
        &self,
        id: impl Into<SharedString>,
        text: impl Into<SharedString>,
        glyph: Option<&str>,
        tone: Tone,
    ) -> gpui_kit::base::Button {
        self.button_face(id.into(), text.into(), glyph, tone, true, None)
    }

    /// A small button with a `size` px glyph (`.btn.small` holding an icon of
    /// that size). Without text it keeps the 8 px padding, as a `.btn.small`
    /// that is not also `.icon`.
    pub(in crate::frontend) fn btn_small_glyph(
        &self,
        id: impl Into<SharedString>,
        text: impl Into<SharedString>,
        glyph: &str,
        size: f32,
        tone: Tone,
    ) -> gpui_kit::base::Button {
        self.button_face(
            id.into(),
            text.into(),
            Some(glyph),
            tone,
            true,
            Some(px(size)),
        )
    }

    fn button_face(
        &self,
        id: SharedString,
        text: SharedString,
        glyph: Option<&str>,
        tone: Tone,
        small: bool,
        glyph_size: Option<Pixels>,
    ) -> gpui_kit::base::Button {
        let p = self.palette;
        let clear = p.background.opacity(0.);
        let (bg, fg, border) = match tone {
            Tone::Plain => (p.control_face, p.ink, p.stroke),
            Tone::Accent => (p.accent, p.on_accent, clear),
            Tone::Danger => (p.danger, p.on_danger, clear),
            Tone::Ghost => (clear, p.ink, clear),
        };
        let icon_only = text.is_empty() && glyph.is_some() && glyph_size.is_none();
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
            .text_size(theme::font(13.))
            .whitespace_nowrap()
            .hover(move |s| match tone {
                Tone::Plain => s.bg(p.control_hover),
                Tone::Ghost => s.bg(p.hover),
                Tone::Accent => s.bg(brighter(p.accent)),
                Tone::Danger => s.bg(brighter(p.danger)),
            })
            .active(move |s| match tone {
                Tone::Plain | Tone::Ghost => s.bg(p.press).text_color(p.muted),
                Tone::Accent => s.bg(p.accent.opacity(0.9)),
                Tone::Danger => s.bg(p.danger.opacity(0.9)),
            })
            .when(tone == Tone::Plain, |b| b.child(control_edge(&p)))
            .accessibility_label(text.clone())
            .when_some(glyph, |b, name| {
                b.child(icon(name, fg).when_some(glyph_size, |i, size| i.size(size)))
            })
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
                    .bg(p.control_face)
                    .text_size(theme::font(12.))
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

    /// A state pill (`.state-pill`): a raised capsule with a coloured dot, or a
    /// spinner while busy.
    pub(in crate::frontend) fn state_pill(&self, text: String, tone: &str) -> AnyElement {
        let p = self.palette;
        let color = match tone {
            "ok" => p.network,
            "warn" => p.io,
            "busy" => p.accent,
            _ => p.faint,
        };
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
            .bg(p.layer_strong)
            .text_size(theme::font(12.))
            .text_color(p.ink)
            .whitespace_nowrap()
            .child(if tone == "busy" {
                self.spinner(10., p.accent)
            } else {
                div()
                    .size(px(8.))
                    .rounded_full()
                    .bg(color)
                    .into_any_element()
            })
            .child(text)
            .into_any_element()
    }

    /// A drop-down: a combo box button as wide as its longest
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
        self.select_with(id, name, options, selected, disabled, false, pick, cx)
    }

    /// A drop-down inside a chart label (`.select.compact`): the label's size
    /// and colour, a face only on hover or while open, as wide as the current
    /// choice; the same list opens under it.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::frontend) fn select_compact(
        &self,
        id: impl Into<SharedString>,
        name: impl Into<SharedString>,
        options: Vec<String>,
        selected: Option<usize>,
        pick: impl Fn(&mut Desktop, usize, &mut Window, &mut Context<Desktop>) + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.select_with(id, name, options, selected, false, true, pick, cx)
    }

    #[allow(clippy::too_many_arguments)]
    fn select_with(
        &self,
        id: impl Into<SharedString>,
        name: impl Into<SharedString>,
        options: Vec<String>,
        selected: Option<usize>,
        disabled: bool,
        compact: bool,
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
                    this.close_select();
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
                        scroll: ScrollHandle::new(),
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
            .flex_shrink_0()
            .justify_start()
            .rounded(px(4.))
            .border_1()
            .hover(move |s| s.bg(p.control_hover))
            .map(|button| {
                if compact {
                    button
                        .min_w_0()
                        .h(px(18.))
                        .ml(px(-4.))
                        .pl(px(4.))
                        .pr(px(2.))
                        .gap(px(2.))
                        .border_color(gpui_kit::gpui::transparent_black())
                        .when(open, |b| b.bg(p.control_hover))
                        .child(
                            div().min_w_0().truncate().child(
                                selected
                                    .and_then(|i| options.get(i))
                                    .cloned()
                                    .unwrap_or_default(),
                            ),
                        )
                } else {
                    button
                        .h(px(32.))
                        .pl(px(11.))
                        .pr(px(10.))
                        .gap(px(10.))
                        .border_color(p.stroke)
                        .bg(if open {
                            p.control_hover
                        } else {
                            p.control_face
                        })
                        .text_color(p.ink)
                        .text_size(theme::font(13.))
                        .active(move |s| s.text_color(p.muted))
                        .child(control_edge(&p))
                        .child(
                            // Every label in one column, the others without
                            // height: the button is as wide as the longest.
                            div().flex_1().flex().flex_col().children(
                                options.iter().enumerate().map(|(index, option)| {
                                    div()
                                        .whitespace_nowrap()
                                        .when(Some(index) != selected, |d| {
                                            d.h_0().overflow_hidden().invisible()
                                        })
                                        .child(option.clone())
                                }),
                            ),
                        )
                }
            })
            .child(icon("ChevronDown", p.muted).size(px(12.)))
            // The button's border box, which the list lines up with: an absolute
            // child fills the padding box (`on_prepaint` would measure from where
            // its probe sits after the padding), plus the 1 px border.
            .child(
                canvas(
                    move |bounds, _, _| {
                        anchors
                            .borrow_mut()
                            .insert(anchor_id, bounds.dilate(px(1.)));
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0(),
            )
            .inert_dimmed(disabled)
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
                            this.close_select();
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
                    "tab" => this.close_select(),
                    _ => return,
                }
                window.refresh();
                cx.notify();
                cx.stop_propagation();
            }))
            .into_any_element()
    }

    /// Closes the open drop-down; its list fades out.
    pub(in crate::frontend) fn close_select(&mut self) {
        if let Some(open) = self.ui.select.take() {
            self.ui.select_leaving = Some((open, Instant::now()));
        }
    }

    /// Takes the active (or given) option of the open drop-down.
    pub(in crate::frontend) fn pick_select(
        &mut self,
        index: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(open) = self.ui.select.clone() else {
            return;
        };
        self.close_select();
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
        let reduce = cx.reduce_motion();
        let leaving = self
            .ui
            .select_leaving
            .as_ref()
            .filter(|(_, at)| !reduce && at.elapsed().as_secs_f32() < 0.083);
        let interactive = self.ui.select.is_some();
        let (open, opacity) = match (&self.ui.select, leaving) {
            // Grows in over 167 ms, decelerating (`flyout-in`; its slight
            // scale is left out).
            (Some(open), _) => (
                open,
                if reduce {
                    1.
                } else {
                    theme::ease_out((open.opened.elapsed().as_secs_f32() / 0.167).min(1.))
                },
            ),
            // Fades out over 83 ms, linear (`fade-out`), without taking input.
            (None, Some((open, at))) => (open, 1. - at.elapsed().as_secs_f32() / 0.083),
            (None, None) => return None,
        };
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
        if opacity < 1. {
            window.request_animation_frame();
        }
        let list = div()
            .id("select-list")
            .track_scroll(&open.scroll)
            .when(interactive, |d| d.occlude())
            .absolute()
            .left(left)
            .top(top)
            .min_w(anchor.size.width)
            .max_h(px(max_height))
            .overflow_y_scroll()
            .p(px(4.))
            // The gutter is kept (`scrollbar-gutter: stable`).
            .pr(px(4. + scrollbar::SIZE))
            .border_1()
            .border_color(p.stroke_strong)
            .rounded(px(8.))
            // No shadow, as the interface's menus and lists (window.md); opaque,
            // to keep the flyout readable over other content.
            .bg(solid(p.flyout))
            .opacity(opacity)
            .text_size(theme::font(13.))
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
            }))
            .child(scrollbar::scrollbar("select-scrollbar", &open.scroll, p.dark).border(1.));
        Some(
            deferred(
                anchored().position(point(px(0.), px(0.))).child(
                    div()
                        .id("select-scrim")
                        .w(viewport.width)
                        .h(viewport.height)
                        .when(interactive, |d| d.occlude())
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, window, cx| {
                                this.close_select();
                                window.refresh();
                                cx.notify();
                            }),
                        )
                        .on_scroll_wheel(cx.listener(|this, _, window, cx| {
                            this.close_select();
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

    /// The new page slides in from the side of its entry in the navigation while
    /// it fades in: from the right for a tab further right, from below
    /// for a page further down the side navigation; the navigation stays put.
    /// Only the opacity and the offset change, never the element tree, so
    /// nothing inside loses its state or replays its own entrance (the details
    /// pane's). Both apply only when drawing (`shift.rs`): the page is laid out
    /// and hit-tested in place, and replays its last paint every frame. The
    /// chart layer takes the opacity and the offset (`page_shift`).
    pub(in crate::frontend) fn enter(
        &self,
        content: impl IntoElement,
        window: &Window,
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
        if !self.page_fading(cx) {
            self.page_opacity.set(1.);
            self.page_shift.set(Point::default());
            return element.into_any_element();
        }
        let progress = theme::ease_out(
            (self.page_switched.elapsed().as_secs_f32() / PAGE_ENTER.as_secs_f32()).min(1.),
        );
        self.page_opacity.set(progress);
        window.request_animation_frame();
        let shift = (1. - progress) * self.page_direction;
        let offset = if self.nav_top {
            point(px(shift * 32.), px(0.))
        } else {
            point(px(0.), px(shift * 24.))
        };
        self.page_shift.set(offset);
        // Drawn moved and faded; laid out in place, so the page (and its cached
        // tables) replays its last paint instead of rendering every frame.
        super::shift::shifted(element, offset, progress).into_any_element()
    }
    /// Whether the page is still entering after a page change.
    pub(in crate::frontend) fn page_fading(&self, cx: &App) -> bool {
        !cx.reduce_motion()
            && self.page_direction != 0.
            && self.page_switched.elapsed() < PAGE_ENTER
    }
}

/// How long a page takes to enter after a page change (300 ms, `EASE_OUT`).
pub(in crate::frontend) const PAGE_ENTER: Duration = Duration::from_millis(300);

/// The shadow of dialogs (`--shadow-flyout`); the interface's menus, lists and
/// toasts have none.
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
