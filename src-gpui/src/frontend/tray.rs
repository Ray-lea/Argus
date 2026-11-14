//! The tray's flyouts: the card that shows while the pointer rests on the
//! tray icon, and the quick panel a right click opens. Both use one window,
//! made ahead of time (when the pointer first reaches the tray icon, when
//! the main window goes to the tray, or when Argus starts there) and kept
//! hidden between uses: making a window costs far more than showing one
//! (a new renderer, swap chain and glyph atlas; in development builds the
//! shaders are compiled too), and a right click must answer at once. They
//! show the desktop's latest sample and recent history and act through the
//! desktop.
//!
//! A flyout is shown fully transparent at the height it last had, laid out,
//! fitted to its content beside the icon (`shell::Anchor`) if that changed,
//! and then faded in by its window's opacity; it fades out the same way and
//! is hidden. Without window motion it appears and goes at once.
use super::*;
use crate::shell::Anchor;
use windows::Win32::Foundation::{COLORREF, HWND, POINT, RECT};
use windows::Win32::Graphics::Dwm::{
    DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::UI::WindowsAndMessaging::{
    GWL_EXSTYLE, GetClientRect, GetWindowLongPtrW, GetWindowRect, HWND_TOPMOST, LWA_ALPHA,
    SWP_HIDEWINDOW, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW,
    SetForegroundWindow, SetLayeredWindowAttributes, SetWindowLongPtrW, SetWindowPos,
    WS_EX_LAYERED,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::frontend) enum Kind {
    Card,
    Panel,
}

impl Kind {
    fn width(self) -> f32 {
        match self {
            Kind::Card => 252.,
            Kind::Panel => 360.,
        }
    }
    /// The height before the content was first laid out.
    fn guess(self) -> f32 {
        match self {
            Kind::Card => 170.,
            Kind::Panel => 480.,
        }
    }
    fn index(self) -> usize {
        self as usize
    }
}

/// A request for the panel this soon after it went by itself is the click
/// that sent it away.
const REOPEN: Duration = Duration::from_millis(500);
/// How long after going to the tray the flyout window is made: after the
/// window's own fade, before the pointer can reach the icon.
const WARM_DELAY: Duration = Duration::from_millis(1500);
/// How far a flyout keeps from the taskbar and the work area's edges.
const GAP: f32 = 12.;
const FADE_IN: f32 = 0.167;
const FADE_OUT: f32 = 0.12;
/// The samples a curve shows.
const HISTORY: usize = 60;

/// One of the busiest processes.
#[derive(Clone)]
pub(in crate::frontend) struct Busy {
    name: String,
    cpu: f64,
    pid: Value,
    create_time: Value,
    image: Option<Arc<Image>>,
}

/// A resource tile: its figure, a reading beside its name, and its recent
/// curve on a 0–1 scale.
#[derive(Clone, Default)]
pub(in crate::frontend) struct Tile {
    figure: String,
    detail: String,
    curve: Vec<f64>,
    /// The performance page's device for it.
    device: String,
}

/// What a flyout shows, taken from the desktop with each sample.
#[derive(Clone, Default)]
pub(in crate::frontend) struct Readings {
    cpu: f64,
    memory: f64,
    gpu: Option<f64>,
    received: String,
    sent: String,
    tiles: [Tile; 4],
    busiest: Vec<Busy>,
    update: Option<String>,
    paused: bool,
    version: String,
    /// A right click opens the menu rather than the panel (the card's hint).
    menu: bool,
}

/// What the flyouts can ask of the desktop.
#[derive(Clone)]
enum Ask {
    Open,
    Update,
    Pause,
    Settings,
    Quit,
    Device(String),
    Reveal(Value, Value),
}

#[derive(Clone, Copy, PartialEq)]
enum Phase {
    /// Not on screen; the window waits for the next flyout.
    Hidden,
    /// Shown fully transparent until its content was laid out.
    Placing,
    Entering(Instant),
    Shown,
    Leaving(Instant),
}

/// The flyout window.
#[derive(Clone)]
pub(in crate::frontend) struct Open {
    window: AnyWindowHandle,
    flyout: Entity<Flyout>,
}

/// The desktop's flyout window, once made.
#[derive(Default)]
pub(in crate::frontend) struct Flyouts {
    open: Option<Open>,
    /// When the panel last went by itself (another window took the
    /// foreground). A right click on the tray icon does that on its way
    /// down, so the click that follows closes rather than reopens.
    panel_left: Option<Instant>,
    /// The window is to be made once the interface is idle.
    warming: bool,
}

pub(in crate::frontend) struct Flyout {
    /// What shows, or showed last.
    kind: Kind,
    anchor: Anchor,
    readings: Readings,
    desktop: WeakEntity<Desktop>,
    desktop_window: AnyWindowHandle,
    host: Host,
    hwnd: HWND,
    phase: Phase,
    fade: bool,
    /// Take the foreground when shown (not in tests: they must not touch
    /// the desktop's focus).
    activate: bool,
    /// The content's height as last laid out, logical pixels.
    measured: Rc<Cell<f32>>,
    /// The height the window was last given.
    placed: f32,
    /// Each kind's height as last laid out: a flyout opens at it, so it
    /// usually needs no second placement.
    heights: [Option<f32>; 2],
    /// Laid out off screen once after the window was made, so the first
    /// panel finds its text shaped, its glyphs drawn and its height known.
    priming: bool,
    focus: FocusHandle,
    _activation: Subscription,
    /// When the flyout was asked for, frames drawn while it faded in, and
    /// whether to report its timings (test instances, `tray-flyout` events).
    requested: Instant,
    frames: u32,
    trace: bool,
}

/// Reports how long a flyout took to reach `phase` since it was asked for.
fn trace(trace: bool, kind: Kind, phase: &str, requested: Instant, frames: u32) {
    if trace {
        println!(
            "{}",
            json!({"event":"tray-flyout","kind":format!("{kind:?}"),"phase":phase,"ms":requested.elapsed().as_secs_f64()*1000.,"frames":frames})
        );
    }
}

fn hwnd_of(window: &Window) -> HWND {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match HasWindowHandle::window_handle(window).map(|handle| handle.as_raw()) {
        Ok(RawWindowHandle::Win32(handle)) => HWND(handle.hwnd.get() as _),
        _ => HWND::default(),
    }
}

/// Makes the window translucent to `alpha` (0–255), or opaque without the
/// layered style.
fn set_alpha(hwnd: HWND, alpha: Option<u8>) {
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let layered = style & WS_EX_LAYERED.0 as isize != 0;
        match alpha {
            Some(alpha) => {
                if !layered {
                    SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style | WS_EX_LAYERED.0 as isize);
                }
                let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), alpha, LWA_ALPHA);
            }
            None if layered => {
                SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style & !(WS_EX_LAYERED.0 as isize));
            }
            None => {}
        }
    }
}

impl Flyout {
    fn new(
        desktop: WeakEntity<Desktop>,
        desktop_window: AnyWindowHandle,
        host: Host,
        activate: bool,
        trace: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let hwnd = hwnd_of(window);
        unsafe {
            let round = DWMWCP_ROUND;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                (&raw const round).cast(),
                size_of_val(&round) as u32,
            );
        }
        // Transparent from the start: the first show lays it out unseen.
        set_alpha(hwnd, Some(0));
        // The panel goes when another window takes the foreground.
        let activation = cx.observe_window_activation(window, |this: &mut Self, window, cx| {
            if this.kind == Kind::Panel
                && this.activate
                && !window.is_window_active()
                && matches!(this.phase, Phase::Entering(_) | Phase::Shown)
            {
                this.close(window, cx);
            }
        });
        Self {
            kind: Kind::Card,
            anchor: Anchor {
                icon: [0; 4],
                work: [0; 4],
                monitor: [0; 4],
                scale: 1.,
            },
            readings: Readings::default(),
            desktop,
            desktop_window,
            host,
            hwnd,
            phase: Phase::Hidden,
            fade: false,
            activate,
            measured: Rc::new(Cell::new(0.)),
            placed: 0.,
            heights: [None; 2],
            priming: false,
            focus: cx.focus_handle(),
            _activation: activation,
            requested: Instant::now(),
            frames: 0,
            trace,
        }
    }

    /// Whether a flyout of `kind` is on screen (or on its way).
    fn showing(&self, kind: Option<Kind>) -> bool {
        !self.priming
            && matches!(
                self.phase,
                Phase::Placing | Phase::Entering(_) | Phase::Shown
            )
            && kind.is_none_or(|kind| kind == self.kind)
    }

    /// Whether a flyout of `kind` is fading out.
    fn leaving(&self, kind: Kind) -> bool {
        matches!(self.phase, Phase::Leaving(_)) && self.kind == kind
    }

    /// Lays the panel out once, transparent and off screen, then hides it.
    fn prime(&mut self, readings: Readings, window: &mut Window, cx: &mut Context<Self>) {
        self.kind = Kind::Panel;
        self.readings = readings;
        self.requested = Instant::now();
        self.priming = true;
        self.phase = Phase::Placing;
        set_alpha(self.hwnd, Some(0));
        self.place(Kind::Panel.guess(), window, cx);
        cx.notify();
    }

    /// Shows the flyout `kind` beside the icon: transparent at the height
    /// it last had, until its content was laid out (`settle`).
    fn show(
        &mut self,
        kind: Kind,
        anchor: Anchor,
        readings: Readings,
        requested: Instant,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.kind = kind;
        self.anchor = anchor;
        self.readings = readings;
        self.requested = requested;
        self.priming = false;
        self.frames = 0;
        self.fade = crate::window_motion::enabled() && crate::shell::animations_enabled();
        self.phase = Phase::Placing;
        set_alpha(self.hwnd, Some(0));
        let height = self.heights[kind.index()].unwrap_or(kind.guess());
        self.place(height, window, cx);
        cx.notify();
    }

    /// Gives the window `height` logical pixels beside the icon, and shows it.
    fn place(&mut self, height: f32, window: &mut Window, cx: &mut Context<Self>) {
        let scale = window.scale_factor();
        let width = (self.kind.width() * scale).round() as i32;
        let tall = (height * scale).round() as i32;
        let (x, y) = if self.priming {
            (-32000, -32000)
        } else {
            self.anchor.place(width, tall, (GAP * scale).round() as i32)
        };
        // The window keeps an invisible frame around its client area: the
        // client area is what shows, so the frame goes around the place.
        let (mut frame, mut client, mut origin) =
            (RECT::default(), RECT::default(), POINT::default());
        unsafe {
            let _ = GetWindowRect(self.hwnd, &mut frame);
            let _ = GetClientRect(self.hwnd, &mut client);
            let _ = ClientToScreen(self.hwnd, &mut origin);
            let _ = SetWindowPos(
                self.hwnd,
                Some(HWND_TOPMOST),
                x - (origin.x - frame.left),
                y - (origin.y - frame.top),
                width + (frame.right - frame.left) - client.right,
                tall + (frame.bottom - frame.top) - client.bottom,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
        }
        self.placed = height;
        // Moving it reported the new size while the window was busy here.
        window.bounds_changed(cx);
        window.refresh();
    }

    /// The content was laid out: fit the window to it, then fade in.
    fn settle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let measured = self.measured.get();
        if measured <= 0. || self.phase == Phase::Hidden {
            return;
        }
        self.heights[self.kind.index()] = Some(measured);
        if (measured - self.placed).abs() > 0.5 {
            // Laid out again at the new size before it shows.
            self.place(measured, window, cx);
            return;
        }
        if self.priming {
            self.priming = false;
            trace(self.trace, self.kind, "primed", self.requested, self.frames);
            self.hide(window, cx);
            return;
        }
        if self.phase == Phase::Placing {
            trace(
                self.trace,
                self.kind,
                "visible",
                self.requested,
                self.frames,
            );
            if self.fade {
                self.phase = Phase::Entering(Instant::now());
            } else {
                set_alpha(self.hwnd, None);
                self.phase = Phase::Shown;
            }
            if self.activate && self.kind == Kind::Panel {
                unsafe {
                    let _ = SetForegroundWindow(self.hwnd);
                }
                self.focus.focus(window, cx);
            }
            window.refresh();
        }
    }

    /// Fades out and hides the window.
    pub(in crate::frontend) fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.phase {
            Phase::Hidden | Phase::Leaving(_) => {}
            Phase::Entering(_) | Phase::Shown if self.fade => {
                self.phase = Phase::Leaving(Instant::now());
                window.refresh();
            }
            _ => self.hide(window, cx),
        }
    }

    /// Takes the window off screen, transparent for the next flyout.
    fn hide(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let kind = self.kind;
        let was_shown = self.phase != Phase::Placing;
        self.phase = Phase::Hidden;
        set_alpha(self.hwnd, Some(0));
        let hwnd = self.hwnd;
        // Shown again before the deferred steps ran: without a fade, a right
        // click closes the card and opens the panel in the same window within
        // one frame. Hiding it then would leave the panel open but unseen.
        let this = cx.weak_entity();
        let reshown = move |cx: &App| {
            this.upgrade()
                .is_some_and(|flyout| flyout.read(cx).phase != Phase::Hidden)
        };
        let reshown_closed = reshown.clone();
        // Hidden after this frame: the window is busy drawing now.
        window.defer(cx, move |window, cx| {
            if reshown(cx) {
                return;
            }
            unsafe {
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    0,
                    0,
                    0,
                    0,
                    SWP_HIDEWINDOW | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            window.bounds_changed(cx);
        });
        let desktop = self.desktop.clone();
        cx.defer(move |cx| {
            if reshown_closed(cx) {
                return;
            }
            let _ = desktop.update(cx, |desktop, _| desktop.tray_closed(kind, was_shown));
        });
    }

    /// Steps the fade; true while it runs.
    fn step(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        match self.phase {
            Phase::Entering(start) => {
                self.frames += 1;
                let t = (start.elapsed().as_secs_f32() / FADE_IN).min(1.);
                if t >= 1. {
                    set_alpha(self.hwnd, None);
                    self.phase = Phase::Shown;
                    trace(self.trace, self.kind, "shown", self.requested, self.frames);
                    return false;
                }
                set_alpha(self.hwnd, Some((theme::ease_out(t) * 255.).round() as u8));
                true
            }
            Phase::Leaving(start) => {
                let t = (start.elapsed().as_secs_f32() / FADE_OUT).min(1.);
                if t >= 1. {
                    self.hide(window, cx);
                    return false;
                }
                set_alpha(
                    self.hwnd,
                    Some(((1. - theme::ease_in(t)) * 255.).round() as u8),
                );
                true
            }
            _ => false,
        }
    }

    fn set_readings(&mut self, readings: Readings, cx: &mut Context<Self>) {
        self.readings = readings;
        cx.notify();
    }

    /// Carries out `ask` through the desktop, in its own window.
    fn ask(&mut self, ask: Ask, window: &mut Window, cx: &mut Context<Self>) {
        let host = self.host.clone();
        let shows = !matches!(ask, Ask::Pause | Ask::Quit);
        if shows {
            crate::shell::show_main(&host);
        }
        match &ask {
            Ask::Quit => host.exit(0),
            Ask::Update => {
                let _ = host.emit("argus-show-update", ());
            }
            Ask::Pause => self.readings.paused = !self.readings.paused,
            _ => {}
        }
        let desktop = self.desktop.clone();
        let handle = self.desktop_window;
        cx.defer(move |cx| {
            let _ = handle.update(cx, |_, window, cx| {
                let _ = desktop.update(cx, |desktop, cx| match ask {
                    Ask::Pause => desktop.act(Action::Pause, window, cx),
                    Ask::Settings => desktop.act(Action::Page("components".into()), window, cx),
                    Ask::Device(device) => desktop.act(Action::Device(device), window, cx),
                    Ask::Reveal(pid, create_time) => desktop.preference(
                        "revealProcess",
                        json!({"pid": pid, "createTime": create_time}),
                        window,
                        cx,
                    ),
                    Ask::Open | Ask::Update | Ask::Quit => {}
                });
            });
        });
        if shows {
            self.close(window, cx);
        } else {
            cx.notify();
        }
    }

    /// A row of a hover state and a click, named for screen readers.
    fn pressable(
        &self,
        id: impl Into<ElementId>,
        name: String,
        ask: Ask,
        hover: Hsla,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        div()
            .id(id)
            .role(Role::Button)
            .aria_label(name)
            .rounded(px(4.))
            .hover(move |s| s.bg(hover))
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                this.ask(ask.clone(), window, cx)
            }))
    }

    fn icon_button(
        &self,
        id: &'static str,
        glyph: &str,
        name: String,
        ask: Ask,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.pressable(id, name, ask, p.hover, cx)
            .size(px(32.))
            .flex()
            .items_center()
            .justify_center()
            .child(icon(glyph, p.ink).size(px(16.)))
            .into_any_element()
    }

    fn card(&self, p: Palette) -> Div {
        let r = &self.readings;
        let bar = |value: f64, color: Hsla| {
            div().h(px(4.)).rounded(px(2.)).bg(p.stroke_strong).child(
                div()
                    .h_full()
                    .w(relative((value / 100.).clamp(0., 1.) as f32))
                    .rounded(px(2.))
                    .bg(color),
            )
        };
        let row = |name: String, value: f64, color: Hsla| {
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .child(div().w(px(40.)).text_color(p.muted).child(name))
                .child(div().flex_1().child(bar(value, color)))
                .child(
                    div()
                        .w(px(40.))
                        .text_right()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(format!("{}%", value.round())),
                )
        };
        let mut rows = vec![
            row(label("common.resource.cpu"), r.cpu, p.cpu),
            row(label("common.resource.memory"), r.memory, p.memory),
        ];
        if let Some(gpu) = r.gpu {
            rows.push(row(label("common.resource.gpu"), gpu, p.gpu));
        }
        div()
            .p(px(12.))
            .flex()
            .flex_col()
            .text_size(theme::font(12.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .mb(px(12.))
                    .child(widgets::logo(16.))
                    .child(div().font_weight(FontWeight::SEMIBOLD).child("Argus"))
                    .when_some(r.update.clone(), |d, version| {
                        d.child(
                            div()
                                .ml_auto()
                                .flex()
                                .items_center()
                                .gap(px(5.))
                                .text_color(p.muted)
                                .child(div().size(px(6.)).rounded_full().bg(p.accent))
                                .child(tr!("components.card.update", version = version)),
                        )
                    }),
            )
            .child(div().flex().flex_col().gap(px(9.)).children(rows))
            .child(
                div()
                    .mt(px(9.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        div()
                            .w(px(40.))
                            .text_color(p.muted)
                            .child(label("common.resource.network")),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_right()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(format!("↓ {}   ↑ {}", r.received, r.sent)),
                    ),
            )
            .child(div().h(px(1.)).mx(px(-12.)).my(px(10.)).bg(p.stroke))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .text_color(p.muted)
                    .child(label("components.card.click"))
                    .child(label(if r.menu {
                        "components.card.rightMenu"
                    } else {
                        "components.card.rightPanel"
                    })),
            )
    }

    fn tile(
        &self,
        index: usize,
        glyph: &str,
        name: String,
        color: Hsla,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tile = self.readings.tiles[index].clone();
        let curve = tile.curve.clone();
        let line = color;
        let area = color.opacity(0.12);
        self.pressable(
            ("tray-tile", index),
            name.clone(),
            Ask::Device(tile.device.clone()),
            p.hover,
            cx,
        )
        .relative()
        .h(px(92.))
        .rounded(px(6.))
        .border_1()
        .border_color(p.stroke)
        .bg(p.layer_strong)
        .overflow_hidden()
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    if curve.len() < 2 {
                        return;
                    }
                    let height = f32::from(bounds.size.height);
                    let width = f32::from(bounds.size.width);
                    let at = |index: usize, value: f64| {
                        point(
                            bounds.origin.x + px(width * index as f32 / (curve.len() - 1) as f32),
                            bounds.origin.y
                                + px(height - 1. - (height - 4.) * value.clamp(0., 1.) as f32),
                        )
                    };
                    let mut fill = PathBuilder::fill();
                    fill.move_to(point(bounds.origin.x, bounds.bottom()));
                    for (index, value) in curve.iter().enumerate() {
                        fill.line_to(at(index, *value));
                    }
                    fill.line_to(point(bounds.right(), bounds.bottom()));
                    fill.close();
                    if let Ok(path) = fill.build() {
                        window.paint_path(path, area);
                    }
                    let mut stroke = PathBuilder::stroke(px(1.5));
                    for (index, value) in curve.iter().enumerate() {
                        if index == 0 {
                            stroke.move_to(at(index, *value));
                        } else {
                            stroke.line_to(at(index, *value));
                        }
                    }
                    if let Ok(path) = stroke.build() {
                        window.paint_path(path, line);
                    }
                },
            )
            .absolute()
            .left_0()
            .right_0()
            .bottom_0()
            .h(px(30.)),
        )
        .child(
            div()
                .relative()
                .flex()
                .items_center()
                .gap(px(6.))
                .px(px(12.))
                .pt(px(10.))
                .text_size(theme::font(12.))
                .text_color(p.muted)
                .child(icon(glyph, color).size(px(14.)))
                .child(name)
                .child(div().ml_auto().truncate().child(tile.detail)),
        )
        .child(
            div()
                .relative()
                .px(px(12.))
                .pt(px(2.))
                .text_size(theme::font(20.))
                .font_weight(FontWeight::SEMIBOLD)
                .child(tile.figure),
        )
        .into_any_element()
    }

    fn panel(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let r = self.readings.clone();
        let header = div()
            .flex()
            .items_center()
            .gap(px(12.))
            .px(px(16.))
            .pt(px(14.))
            .pb(px(12.))
            .child(widgets::logo(28.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_size(theme::font(14.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Argus"),
                    )
                    .child(
                        div()
                            .text_size(theme::font(12.))
                            .text_color(p.muted)
                            .child(r.version.clone()),
                    ),
            );
        let update = r.update.clone().map(|version| {
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .mx(px(12.))
                .mb(px(12.))
                .pl(px(12.))
                .pr(px(10.))
                .py(px(10.))
                .rounded(px(6.))
                .bg(p.accent.opacity(0.1))
                .child(icon("DownloadIcon", p.accent).size(px(16.)))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(tr!("components.panel.update", version = version))
                        .child(
                            div()
                                .text_size(theme::font(12.))
                                .text_color(p.muted)
                                .child(label("components.panel.updateHint")),
                        ),
                )
                .child(
                    self.pressable(
                        "tray-update",
                        label("components.panel.install"),
                        Ask::Update,
                        p.hover,
                        cx,
                    )
                    .h(px(28.))
                    .px(px(12.))
                    .flex()
                    .items_center()
                    .border_1()
                    .border_color(p.stroke_strong)
                    .bg(p.control_face)
                    .child(label("components.panel.install")),
                )
        });
        let tiles = div()
            .grid()
            .grid_cols(2)
            .gap(px(8.))
            .px(px(12.))
            .child(self.tile(0, "CpuIcon", label("common.resource.cpu"), p.cpu, p, cx))
            .child(self.tile(
                1,
                "MemoryIcon",
                label("common.resource.memory"),
                p.memory,
                p,
                cx,
            ))
            .child(self.tile(2, "GpuIcon", label("common.resource.gpu"), p.gpu, p, cx))
            .child(self.tile(
                3,
                "NetworkIcon",
                label("common.resource.network"),
                p.network,
                p,
                cx,
            ));
        let rows = r
            .busiest
            .iter()
            .enumerate()
            .map(|(index, busy)| {
                self.pressable(
                    ("tray-process", index),
                    busy.name.clone(),
                    Ask::Reveal(busy.pid.clone(), busy.create_time.clone()),
                    p.hover,
                    cx,
                )
                .h(px(32.))
                .mx(px(4.))
                .px(px(10.))
                .flex()
                .items_center()
                .gap(px(10.))
                .child(match busy.image.clone() {
                    Some(image) => img(image).size(px(16.)).flex_shrink_0().into_any_element(),
                    None => icon("AppIcon", p.muted)
                        .size(px(16.))
                        .flex_shrink_0()
                        .into_any_element(),
                })
                .child(div().flex_1().min_w_0().truncate().child(busy.name.clone()))
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(crate::regional::number!("{:.1}%", busy.cpu)),
                )
            })
            .collect::<Vec<_>>();
        let open = self
            .pressable(
                "tray-open",
                label("components.panel.open"),
                Ask::Open,
                p.accent.opacity(0.9),
                cx,
            )
            .h(px(32.))
            .px(px(14.))
            .flex()
            .items_center()
            .gap(px(8.))
            .bg(p.accent)
            .text_color(p.on_accent)
            .child(icon("AppIcon", p.on_accent).size(px(16.)))
            .child(label("components.panel.open"));
        let (pause_glyph, pause_name) = if r.paused {
            ("PlayIcon", label("shell.resume"))
        } else {
            ("PauseIcon", label("shell.pause"))
        };
        div()
            .flex()
            .flex_col()
            .child(header)
            .children(update)
            .child(tiles)
            .child(
                div()
                    .flex()
                    .justify_between()
                    .px(px(16.))
                    .pt(px(14.))
                    .pb(px(6.))
                    .text_size(theme::font(12.))
                    .text_color(p.muted)
                    .child(
                        div()
                            .font_weight(theme::LABEL_WEIGHT)
                            .child(label("components.panel.busiest")),
                    )
                    .child(label("common.resource.cpu")),
            )
            .child(div().flex().flex_col().pb(px(8.)).children(rows))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .px(px(12.))
                    .py(px(10.))
                    .border_t_1()
                    .border_color(p.stroke)
                    .bg(p.layer)
                    .child(open)
                    .child(div().flex_1())
                    .child(self.icon_button(
                        "tray-pause",
                        pause_glyph,
                        pause_name,
                        Ask::Pause,
                        p,
                        cx,
                    ))
                    .child(self.icon_button(
                        "tray-settings",
                        "SettingsIcon",
                        label("components.panel.settings"),
                        Ask::Settings,
                        p,
                        cx,
                    ))
                    .child(self.icon_button(
                        "tray-quit",
                        "SignOutIcon",
                        label("components.panel.quit"),
                        Ask::Quit,
                        p,
                        cx,
                    )),
            )
    }
}

impl Render for Flyout {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.phase == Phase::Hidden {
            return div().id("tray-flyout").size_full();
        }
        if self.step(window, cx) {
            window.request_animation_frame();
        }
        let p = Palette::current(cx);
        let measured = self.measured.clone();
        let owner = cx.entity().downgrade();
        let placing = self.phase == Phase::Placing;
        let content = match self.kind {
            Kind::Card => self.card(p),
            Kind::Panel => self.panel(p, cx),
        };
        div()
            .id("tray-flyout")
            .role(Role::Dialog)
            .aria_label(label("components.panel.label"))
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    this.close(window, cx);
                }
            }))
            .size_full()
            .overflow_hidden()
            .bg(widgets::solid(p.flyout))
            .text_color(p.ink)
            .text_size(theme::font(13.))
            .line_height(theme::line_height())
            // A column: the content keeps its own height, which the window follows.
            .flex()
            .flex_col()
            .child(
                content.relative().flex_shrink_0().child(
                    canvas(
                        move |bounds, window, cx| {
                            let height = f32::from(bounds.size.height);
                            // Placing, every layout settles; shown, a change of height.
                            if (measured.replace(height) - height).abs() > 0.5 || placing {
                                let owner = owner.clone();
                                window.defer(cx, move |window, cx| {
                                    let _ = owner.update(cx, |this, cx| this.settle(window, cx));
                                });
                            }
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .inset_0(),
                ),
            )
    }
}

impl Desktop {
    /// What the flyouts show now.
    fn tray_readings(&self) -> Readings {
        let sys = &self.snapshot["system"];
        let hw = &self.snapshot["hardware"];
        let bits = self.prefs.get::<String>("netUnit", "bytes".into()) == "bits";
        let history = |key: &str| -> Vec<f64> {
            self.timeline
                .samples
                .get(key)
                .map(|samples| {
                    let skip = samples.len().saturating_sub(HISTORY);
                    samples
                        .iter()
                        .skip(skip)
                        .map(|(_, value)| value.unwrap_or(0.))
                        .collect()
                })
                .unwrap_or_default()
        };
        let mem_total = number(sys, "memTotal").max(1.);
        let mem_used = number(sys, "memUsed");
        let gpus = values(&hw["gpus"]);
        let gpu = gpus
            .iter()
            .fold(None::<&Value>, |busiest, g| match busiest {
                Some(b) if number(g, "util") <= number(b, "util") => Some(b),
                _ => Some(g),
            });
        let nics = values(&hw["nics"]);
        let received = nics.iter().map(|n| number(n, "rx")).sum::<f64>();
        let sent = nics.iter().map(|n| number(n, "tx")).sum::<f64>();
        let rate = |value: f64| {
            if value < 1. {
                if bits { "0 Kbps" } else { "0 KB/s" }.to_owned()
            } else {
                theme::network(value, bits)
            }
        };
        let network = history("totals.network");
        let network_top = network.iter().copied().fold(1., f64::max);
        let gb = |bytes: f64| crate::regional::number!("{:.1}", bytes / 1073741824.);
        let tiles = [
            Tile {
                figure: format!("{}%", number(sys, "cpu").round()),
                detail: String::new(),
                curve: history("system.cpu").iter().map(|v| v / 100.).collect(),
                device: "cpu".into(),
            },
            Tile {
                figure: format!("{}%", (mem_used / mem_total * 100.).round()),
                detail: format!("{} / {} GB", gb(mem_used), gb(mem_total)),
                curve: history("system.memUsed")
                    .iter()
                    .map(|v| v / mem_total)
                    .collect(),
                device: "memory".into(),
            },
            Tile {
                figure: gpu
                    .map(|g| format!("{}%", number(g, "util").round()))
                    .unwrap_or_else(|| label("format.missing")),
                detail: String::new(),
                curve: history("totals.gpu").iter().map(|v| v / 100.).collect(),
                device: gpu.map_or_else(
                    || "cpu".into(),
                    |g| format!("gpu:{}", text(&g["info"], "id")),
                ),
            },
            Tile {
                figure: format!("↓ {}", rate(received)),
                detail: format!("↑ {}", rate(sent)),
                curve: network.iter().map(|v| v / network_top).collect(),
                device: format!(
                    "nic:{}",
                    nics.first()
                        .map(|n| text(&n["info"], "id"))
                        .unwrap_or_default()
                ),
            },
        ];
        let mut processes = values(&self.snapshot["processes"])
            .iter()
            .filter(|p| number(p, "pid") > 0.)
            .collect::<Vec<_>>();
        processes.sort_by(|a, b| number(b, "cpu").total_cmp(&number(a, "cpu")));
        let busiest = processes
            .into_iter()
            .take(3)
            .map(|p| Busy {
                name: Some(text(p, "description"))
                    .filter(|d| !d.trim().is_empty())
                    .unwrap_or_else(|| text(p, "name"))
                    .to_owned(),
                cpu: number(p, "cpu"),
                pid: p["pid"].clone(),
                create_time: p["createTime"].clone(),
                image: p["icon"]
                    .as_u64()
                    .and_then(|id| self.process_image(id as u32)),
            })
            .collect();
        let update = self
            .data
            .get("updates")
            .filter(|s| matches!(text(s, "state"), "ready" | "available"))
            .and_then(|s| s["version"].as_str())
            .map(str::to_owned);
        Readings {
            cpu: number(sys, "cpu"),
            memory: mem_used / mem_total * 100.,
            gpu: gpu.map(|g| number(g, "util")),
            received: rate(received),
            sent: rate(sent),
            tiles,
            busiest,
            update,
            paused: self.paused,
            version: format!("v{}", self.shown_version()),
            menu: self.data["settings"]["trayMenu"].as_str() == Some("menu"),
        }
    }

    /// Whether the tray uses a flyout at all: the hover card, or the panel
    /// on a right click.
    fn tray_flyouts_used(&self) -> bool {
        let settings = &self.data["settings"];
        settings["trayCard"].as_bool().unwrap_or(true)
            || settings["trayMenu"].as_str() != Some("menu")
    }

    /// Makes the flyout window, hidden, unless it exists: a flyout then only
    /// has to be shown.
    pub(in crate::frontend) fn warm_tray(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tray.open.is_some() || !self.tray_flyouts_used() {
            return;
        }
        let started = Instant::now();
        let desktop = cx.entity().downgrade();
        let desktop_window = window.window_handle();
        let host = self.host.clone();
        let (activate, test) = (!self.options.test, self.options.test);
        let options = WindowOptions {
            titlebar: None,
            focus: false,
            show: false,
            kind: WindowKind::PopUp,
            is_movable: false,
            is_resizable: false,
            is_minimizable: false,
            window_background: WindowBackgroundAppearance::Opaque,
            // The flyouts are seldom the active window (the card never is): their
            // fades keep the display's pace, as the main window does.
            inactive_frame_interval: None,
            ..Default::default()
        };
        let opened = gpui_kit::open_window(options, cx, move |window, cx| {
            cx.new(|cx| Flyout::new(desktop, desktop_window, host, activate, test, window, cx))
        });
        match opened {
            Ok((window, flyout)) => {
                if test {
                    println!(
                        "{}",
                        json!({"event":"tray-flyout","phase":"made","ms":started.elapsed().as_secs_f64()*1000.})
                    );
                }
                let readings = self.tray_readings();
                let _ = window.update(cx, |_, window, cx| {
                    flyout.update(cx, |flyout, cx| flyout.prime(readings, window, cx))
                });
                self.tray.open = Some(Open { window, flyout });
            }
            Err(error) => argus_core::diagnostics::warn(
                "tray flyout creation",
                argus_core::diagnostics::error_chain(error.as_ref()),
            ),
        }
    }

    /// The shell asks for the flyout window ahead of use: now when a flyout
    /// may be asked for in a moment, otherwise a little later, when the
    /// window has gone to the tray or Argus has just started there.
    pub(in crate::frontend) fn schedule_tray_warm(
        &mut self,
        soon: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.tray.open.is_some() {
            return;
        }
        if soon {
            self.warm_tray(window, cx);
            return;
        }
        if std::mem::replace(&mut self.tray.warming, true) {
            return;
        }
        cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(WARM_DELAY).await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.tray.warming = false;
                this.warm_tray(window, cx);
            });
        })
        .detach();
    }

    /// Gives the flyout window back, unless a flyout shows: Argus has been
    /// hidden long enough to return its memory (`memory.releaseHidden`).
    pub(in crate::frontend) fn release_tray(&mut self, cx: &mut Context<Self>) {
        let Some(Open { window, flyout }) = self.tray.open.clone() else {
            return;
        };
        if flyout.read(cx).showing(None) {
            return;
        }
        self.tray.open = None;
        let _ = window.update(cx, |_, window, _| window.remove_window());
    }

    /// Shows the flyout `kind`, making its window first if need be.
    fn open_tray(
        &mut self,
        kind: Kind,
        anchor: Anchor,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let requested = Instant::now();
        self.warm_tray(window, cx);
        let Some(Open { window, flyout }) = self.tray.open.clone() else {
            return;
        };
        let readings = self.tray_readings();
        let _ = window.update(cx, |_, window, cx| {
            flyout.update(cx, |flyout, cx| {
                flyout.show(kind, anchor, readings, requested, window, cx)
            })
        });
        crate::shell::set_flyout_open(true);
        // A fresh sample now; the sampler keeps the shown window's pace meanwhile.
        self.host
            .state::<AppState>()
            .sampler
            .send(crate::sampler::Control::Refresh);
    }

    /// Closes the flyout, if `kind` (or any) shows, with its fade.
    fn close_tray(&mut self, kind: Option<Kind>, cx: &mut App) {
        if let Some(Open { window, flyout }) = self.tray.open.clone()
            && flyout.read(cx).showing(kind)
        {
            let _ = window.update(cx, |_, window, cx| {
                flyout.update(cx, |flyout, cx| flyout.close(window, cx))
            });
        }
    }

    /// The flyout has gone: the sampler slows down again, and a panel that
    /// went by itself remembers when.
    fn tray_closed(&mut self, kind: Kind, was_shown: bool) {
        crate::shell::set_flyout_open(false);
        if kind == Kind::Panel && was_shown {
            self.tray.panel_left = Some(Instant::now());
        }
    }

    /// The tray asks for the quick panel: it opens, or closes when open.
    pub(in crate::frontend) fn tray_panel(
        &mut self,
        anchor: &Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (panel, leaving) = self.tray.open.as_ref().map_or((false, false), |open| {
            let flyout = open.flyout.read(cx);
            (
                flyout.showing(Some(Kind::Panel)),
                flyout.leaving(Kind::Panel),
            )
        });
        if panel {
            self.tray.panel_left = None;
            self.close_tray(Some(Kind::Panel), cx);
            return;
        }
        // Going away because this click took the foreground from it.
        if leaving {
            return;
        }
        if self
            .tray
            .panel_left
            .take()
            .is_some_and(|left| left.elapsed() < REOPEN)
        {
            return;
        }
        let Ok(anchor) = serde_json::from_value::<Anchor>(anchor.clone()) else {
            return;
        };
        self.open_tray(Kind::Panel, anchor, window, cx);
    }

    /// Closes the quick panel if it is open.
    pub(in crate::frontend) fn close_tray_panel(&mut self, cx: &mut Context<Self>) {
        self.close_tray(Some(Kind::Panel), cx);
    }

    /// The pointer rests on the tray icon, or left it.
    pub(in crate::frontend) fn tray_card(
        &mut self,
        event: &Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event["open"].as_bool() != Some(true) {
            self.close_tray(Some(Kind::Card), cx);
            return;
        }
        // The panel, or the card already, shows.
        if self
            .tray
            .open
            .as_ref()
            .is_some_and(|open| open.flyout.read(cx).showing(None))
        {
            return;
        }
        let Ok(anchor) = serde_json::from_value::<Anchor>(event["anchor"].clone()) else {
            return;
        };
        self.open_tray(Kind::Card, anchor, window, cx);
    }

    /// A new sample, an update or a pause: a flyout that shows follows.
    pub(in crate::frontend) fn refresh_tray(&mut self, cx: &mut Context<Self>) {
        let Some(open) = self.tray.open.clone() else {
            return;
        };
        if !open.flyout.read(cx).showing(None) {
            return;
        }
        let readings = self.tray_readings();
        open.flyout
            .update(cx, |flyout, cx| flyout.set_readings(readings, cx));
    }
}
