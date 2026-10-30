//! Shared scrollbars: a 10 px transparent track and a 4 px rounded thumb.
//! Hover and dragging widen the thumb; scrolling uses the view's scroll handle.
use super::*;
use gpui_kit::Axis;

/// The gutter (`--scrollbar-size`).
pub(in crate::frontend) const SIZE: f32 = 10.;
/// The shortest thumb: Blink gives an auto-sized custom thumb the platform
/// scrollbar's thickness, 17 px on Windows.
const MIN_THUMB: f32 = 17.;
/// Chromium's page step, as a share of the visible length (`kFractionToStepWhenPaging`).
const PAGE: f32 = 0.875;
/// A held track press repeats after this, then at the repeat interval (Blink's
/// autoscroll timer, which a held spin button uses too).
pub(in crate::frontend) const REPEAT_DELAY: Duration = Duration::from_millis(250);
pub(in crate::frontend) const REPEAT_INTERVAL: Duration = Duration::from_millis(50);

thread_local! {
    static TRACE: Cell<Option<(Instant, Duration)>> = const { Cell::new(None) };
}

/// Test runs: every scrollbar prints its offset and thumb on each frame for `ms`.
pub(in crate::frontend) fn trace(ms: u64) {
    TRACE.set(Some((Instant::now(), Duration::from_millis(ms))));
}

/// Milliseconds into a running trace.
pub(in crate::frontend) fn trace_ms() -> Option<f64> {
    let (start, length) = TRACE.get()?;
    let elapsed = start.elapsed();
    (elapsed < length).then_some(elapsed.as_secs_f64() * 1000.)
}

/// What the pointer is doing with a scrollbar, kept across frames.
#[derive(Default)]
struct BarState {
    hovered: bool,
    /// While the thumb is held: where in the thumb it was taken.
    grab: Option<Pixels>,
    /// While the track is held: the pointer, and the repeating page steps.
    press: Option<(Point<Pixels>, Task<()>)>,
}

/// The bar's geometry this frame.
#[derive(Clone, Copy)]
struct Geometry {
    axis: Axis,
    track: Bounds<Pixels>,
    thumb: Bounds<Pixels>,
    visible: Pixels,
    max: Pixels,
}

impl Geometry {
    fn along(&self, point: Point<Pixels>) -> Pixels {
        match self.axis {
            Axis::Vertical => point.y,
            Axis::Horizontal => point.x,
        }
    }

    fn start(&self, bounds: Bounds<Pixels>) -> Pixels {
        self.along(bounds.origin)
    }

    fn length(&self, bounds: Bounds<Pixels>) -> Pixels {
        match self.axis {
            Axis::Vertical => bounds.size.height,
            Axis::Horizontal => bounds.size.width,
        }
    }

    /// The scroll offset (positive, along the axis) that puts the thumb's start at `start`.
    fn offset_for_thumb(&self, start: Pixels) -> Pixels {
        let travel = self.length(self.track) - self.length(self.thumb);
        if travel <= px(0.) {
            return px(0.);
        }
        ((start - self.start(self.track)) / travel * self.max).clamp(px(0.), self.max)
    }
}

/// A scrollbar over the scroller that `handle` tracks.
pub(in crate::frontend) struct Scrollbar {
    id: ElementId,
    handle: ScrollHandle,
    axis: Axis,
    /// `::-webkit-scrollbar-track { margin }` at the track's start and end.
    margin: (Pixels, Pixels),
    /// The scroller's border, which the bar sits inside.
    border: Pixels,
    /// The view that paints the scrolled element, when another view paints the bar.
    view: Option<EntityId>,
    /// A horizontal bar in its own rail just below this scroller (the process
    /// table's `.hscroll`), rather than over the scroller's bottom edge.
    rail_below: Option<ScrollHandle>,
    dark: bool,
}

/// A vertical scrollbar for the scroller tracked by `handle`.
pub(in crate::frontend) fn scrollbar(
    id: impl Into<ElementId>,
    handle: &ScrollHandle,
    dark: bool,
) -> Scrollbar {
    Scrollbar {
        id: id.into(),
        handle: handle.clone(),
        axis: Axis::Vertical,
        margin: (px(0.), px(0.)),
        border: px(0.),
        view: None,
        rail_below: None,
        dark,
    }
}

impl Scrollbar {
    pub fn horizontal(mut self) -> Self {
        self.axis = Axis::Horizontal;
        self
    }

    /// Track margin at both ends (`::-webkit-scrollbar-track { margin: N 0 }`).
    pub fn track_margin(mut self, margin: f32) -> Self {
        self.margin = (px(margin), px(margin));
        self
    }

    /// The scroller's border width, which the gutter sits inside.
    pub fn border(mut self, width: f32) -> Self {
        self.border = px(width);
        self
    }

    /// The view to redraw when the bar scrolls, if not the one painting the bar.
    /// A horizontal bar in a rail of its own just below `above`'s scroller.
    pub fn rail_below(mut self, above: &ScrollHandle) -> Self {
        self.axis = Axis::Horizontal;
        self.rail_below = Some(above.clone());
        self
    }

    pub fn view(mut self, view: EntityId) -> Self {
        self.view = Some(view);
        self
    }

    fn geometry(&self) -> Option<Geometry> {
        let viewport = self.handle.bounds();
        let max = self.handle.max_offset();
        let offset = self.handle.offset();
        let border = self.border;
        let (length, max, offset) = match self.axis {
            Axis::Vertical => (viewport.size.height, max.y, -offset.y),
            Axis::Horizontal => (viewport.size.width, max.x, -offset.x),
        };
        // Chromium shows no bar for an overflow under half a pixel either.
        if max < px(0.5) || length <= px(0.) {
            return None;
        }
        let visible = length - border * 2.;
        let track = match self.axis {
            Axis::Vertical => Bounds::new(
                point(
                    viewport.right() - border - px(SIZE),
                    viewport.top() + border + self.margin.0,
                ),
                size(px(SIZE), visible - self.margin.0 - self.margin.1),
            ),
            Axis::Horizontal => Bounds::new(
                point(
                    viewport.left() + border + self.margin.0,
                    match &self.rail_below {
                        Some(above) => above.bounds().bottom(),
                        None => viewport.bottom() - border - px(SIZE),
                    },
                ),
                size(visible - self.margin.0 - self.margin.1, px(SIZE)),
            ),
        };
        let track_length = f32::from(match self.axis {
            Axis::Vertical => track.size.height,
            Axis::Horizontal => track.size.width,
        })
        .floor();
        if track_length <= 0. {
            return None;
        }
        // Blink: the thumb's share of the track is the visible share of the
        // content, rounded; its position is truncated, but never 0 once scrolled.
        let total = f32::from(visible + max);
        let thumb_length = (f32::from(visible) / total * track_length)
            .round()
            .max(MIN_THUMB)
            .min(track_length);
        let position = (f32::from(offset.clamp(px(0.), max)) * (track_length - thumb_length)
            / f32::from(max))
        .max(0.);
        let position = if position > 0. && position < 1. {
            1.
        } else {
            position.trunc()
        };
        let thumb = match self.axis {
            Axis::Vertical => Bounds::new(
                point(track.left(), track.top() + px(position)),
                size(px(SIZE), px(thumb_length)),
            ),
            Axis::Horizontal => Bounds::new(
                point(track.left() + px(position), track.top()),
                size(px(thumb_length), px(SIZE)),
            ),
        };
        Some(Geometry {
            axis: self.axis,
            track,
            thumb,
            visible,
            max,
        })
    }
}

impl IntoElement for Scrollbar {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

pub(in crate::frontend) struct ScrollbarPrepaint {
    geometry: Geometry,
    hitbox: Hitbox,
    state: Rc<RefCell<BarState>>,
}

impl Element for Scrollbar {
    type RequestLayoutState = ();
    type PrepaintState = Option<ScrollbarPrepaint>;

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        // No room taken, so the scrolled content's size is the content's own.
        let mut style = Style {
            position: Position::Absolute,
            ..Default::default()
        };
        style.inset.top = px(0.).into();
        style.inset.left = px(0.).into();
        style.size.width = px(0.).into();
        style.size.height = px(0.).into();
        (window.request_layout(style, None, cx), ())
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        _: &mut App,
    ) -> Self::PrepaintState {
        let state = window.with_element_state(id?, |state: Option<Rc<RefCell<BarState>>>, _| {
            let state = state.unwrap_or_default();
            (state.clone(), state)
        });
        if let Some(ms) = trace_ms() {
            let geometry = self.geometry();
            println!(
                "{}",
                json!({"event":"scroll","id":self.id.to_string(),"ms":ms,
                "offset":f32::from(-self.handle.offset().y),"max":f32::from(self.handle.max_offset().y),
                "view":f32::from(self.handle.bounds().size.height),
                "maxX":f32::from(self.handle.max_offset().x),"viewW":f32::from(self.handle.bounds().size.width),
                "thumb":geometry.map(|g| [f32::from(g.thumb.left()), f32::from(g.thumb.top()), f32::from(g.thumb.size.width), f32::from(g.thumb.size.height)])})
            );
        }
        let Some(geometry) = self.geometry() else {
            // Nothing to scroll: a held thumb or track lets go.
            *state.borrow_mut() = BarState::default();
            return None;
        };
        // Over the bar the content gets no hover or clicks, but the wheel still scrolls it.
        let hitbox = window.insert_hitbox(geometry.track, HitboxBehavior::BlockMouseExceptScroll);
        Some(ScrollbarPrepaint {
            geometry,
            hitbox,
            state,
        })
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        _: &mut App,
    ) {
        let Some(ScrollbarPrepaint {
            geometry,
            hitbox,
            state,
        }) = prepaint.take()
        else {
            return;
        };
        let active = {
            let state = state.borrow();
            state.hovered || state.grab.is_some()
        };
        // `border: 3px solid transparent` (2 px when active) with `border-radius:
        // 10px`, which CSS scales down to half the box's narrower side.
        let inset = px(if active { 2. } else { 3. });
        let thumb = geometry.thumb;
        let outer = (thumb.size.width.min(thumb.size.height) / 2.).min(px(10.));
        let fill = Bounds::new(
            thumb.origin + point(inset, inset),
            size(
                thumb.size.width - inset * 2.,
                thumb.size.height - inset * 2.,
            ),
        );
        let color = thumb_color(self.dark, active);
        if fill.size.width > px(0.) && fill.size.height > px(0.) {
            window
                .paint_quad(gpui_kit::fill(fill, color).corner_radii((outer - inset).max(px(0.))));
        }
        window.set_cursor_style(CursorStyle::Arrow, &hitbox);

        let painter = window.current_view();
        let scroller = self.view.unwrap_or(painter);
        let notify = move |cx: &mut App| {
            cx.notify(painter);
            if scroller != painter {
                cx.notify(scroller);
            }
        };
        let handle = self.handle.clone();
        let axis = geometry.axis;
        let set_along = move |handle: &ScrollHandle, value: Pixels| {
            let mut offset = handle.offset();
            match axis {
                Axis::Vertical => offset.y = -value,
                Axis::Horizontal => offset.x = -value,
            }
            handle.set_offset(offset);
        };

        window.on_mouse_event({
            let state = state.clone();
            let hitbox = hitbox.clone();
            let handle = handle.clone();
            move |event: &MouseMoveEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble {
                    return;
                }
                let mut state = state.borrow_mut();
                if let Some(grab) = state.grab {
                    if event.pressed_button == Some(MouseButton::Left) {
                        let start = geometry.along(event.position) - grab;
                        set_along(&handle, geometry.offset_for_thumb(start));
                        notify(cx);
                        return;
                    }
                    state.grab = None;
                    notify(cx);
                }
                if let Some((pointer, _)) = state.press.as_mut() {
                    *pointer = event.position;
                }
                let hovered = hitbox.is_hovered(window) && geometry.thumb.contains(&event.position);
                if hovered != state.hovered {
                    state.hovered = hovered;
                    notify(cx);
                }
            }
        });

        window.on_mouse_event({
            let state = state.clone();
            let hitbox = hitbox.clone();
            let handle = handle.clone();
            move |event: &MouseDownEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble || !hitbox.is_hovered(window) {
                    return;
                }
                cx.stop_propagation();
                if event.button != MouseButton::Left {
                    return;
                }
                let mut bar = state.borrow_mut();
                if geometry.thumb.contains(&event.position) {
                    bar.grab =
                        Some(geometry.along(event.position) - geometry.start(geometry.thumb));
                    bar.hovered = true;
                    notify(cx);
                    return;
                }
                // A track press pages toward the pointer, then repeats while held
                // until the thumb reaches it.
                page_toward(&handle, geometry, event.position);
                notify(cx);
                let repeat = {
                    let state = state.clone();
                    let handle = handle.clone();
                    window.spawn(cx, async move |cx| {
                        cx.background_executor().timer(REPEAT_DELAY).await;
                        loop {
                            let pointer = state.borrow().press.as_ref().map(|(p, _)| *p);
                            let Some(pointer) = pointer else {
                                break;
                            };
                            let moved = cx
                                .update(|_, cx| {
                                    let moved = page_toward(&handle, geometry, pointer);
                                    if moved {
                                        notify(cx);
                                    }
                                    moved
                                })
                                .unwrap_or(false);
                            if !moved {
                                break;
                            }
                            cx.background_executor().timer(REPEAT_INTERVAL).await;
                        }
                    })
                };
                bar.press = Some((event.position, repeat));
            }
        });

        window.on_mouse_event({
            let state = state.clone();
            move |event: &MouseUpEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble || event.button != MouseButton::Left {
                    return;
                }
                let mut state = state.borrow_mut();
                if state.grab.is_none() && state.press.is_none() {
                    return;
                }
                state.grab = None;
                state.press = None;
                state.hovered =
                    hitbox.is_hovered(window) && geometry.thumb.contains(&event.position);
                notify(cx);
            }
        });
    }
}

/// Pages toward `pointer` from where the offset is heading, unless the thumb
/// will already be under it. Returns whether it moved.
fn page_toward(handle: &ScrollHandle, geometry: Geometry, pointer: Point<Pixels>) -> bool {
    let target = handle.wheel_target();
    let target_along = match geometry.axis {
        Axis::Vertical => -target.y,
        Axis::Horizontal => -target.x,
    };
    // Where the thumb will be once the running page step lands.
    let travel = geometry.length(geometry.track) - geometry.length(geometry.thumb);
    let start = geometry.start(geometry.track)
        + if geometry.max > px(0.) {
            travel * (target_along / geometry.max).clamp(0., 1.)
        } else {
            px(0.)
        };
    let end = start + geometry.length(geometry.thumb);
    let at = geometry.along(pointer);
    let step = geometry.visible * PAGE;
    let next = if at < start {
        target_along - step
    } else if at >= end {
        target_along + step
    } else {
        return false;
    };
    let to = match geometry.axis {
        Axis::Vertical => point(target.x, -next),
        Axis::Horizontal => point(-next, target.y),
    };
    handle.glide_to(to)
}

fn thumb_color(dark: bool, active: bool) -> Hsla {
    thread_local! {
        static COLORS: RefCell<HashMap<(bool, bool), Hsla>> = RefCell::new(HashMap::new());
    }
    COLORS.with_borrow_mut(|colors| {
        *colors.entry((dark, active)).or_insert_with(|| {
            theme::token(
                if active {
                    "scroll-thumb-hover"
                } else {
                    "scroll-thumb"
                },
                dark,
            )
        })
    })
}

/// A scroll handle that lives as long as the program, for a scroller with a
/// fixed name whose builder has no window state to keep one in.
pub(in crate::frontend) fn keyed_handle(name: &str) -> ScrollHandle {
    thread_local! {
        static HANDLES: RefCell<HashMap<String, ScrollHandle>> = RefCell::new(HashMap::new());
    }
    HANDLES.with_borrow_mut(|handles| handles.entry(name.to_owned()).or_default().clone())
}

/// A scroll handle for a scroller that starts at the top again whenever
/// `generation` changes (another dialog, another step).
pub(in crate::frontend) fn fresh_handle(name: &'static str, generation: u64) -> ScrollHandle {
    thread_local! {
        static FRESH: RefCell<HashMap<&'static str, (u64, ScrollHandle)>> = RefCell::new(HashMap::new());
    }
    FRESH.with_borrow_mut(|handles| {
        let entry = handles
            .entry(name)
            .or_insert_with(|| (generation, ScrollHandle::new()));
        if entry.0 != generation {
            *entry = (generation, ScrollHandle::new());
        }
        entry.1.clone()
    })
}

/// Wheel notches over a scroller whose own wheel handling jumps (the tables'
/// virtual lists) glide instead, as everywhere else. Paint it before the
/// scroller: it takes the notch in the capture phase. `view` paints the scroller.
pub(in crate::frontend) fn wheel_glide(handle: &ScrollHandle, view: EntityId) -> impl IntoElement {
    let handle = handle.clone();
    canvas(
        |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
        move |_, hitbox, window, _| {
            window.on_mouse_event(move |event: &ScrollWheelEvent, phase, window, cx| {
                if phase != DispatchPhase::Capture
                    || event.touch_phase != TouchPhase::Moved
                    || !hitbox.should_handle_scroll(window)
                    || !handle.bounds().contains(&event.position)
                {
                    return;
                }
                // Lines come from a wheel; Shift turns them sideways, left to the table.
                let ScrollDelta::Lines(lines) = event.delta else {
                    return;
                };
                if lines.y == 0. {
                    return;
                }
                if handle.scroll_by_lines(point(0., lines.y)) {
                    cx.notify(view);
                    cx.stop_propagation();
                } else if handle.wheel_target() != handle.offset() {
                    // A notch past the edge while the glide is still on its way
                    // there belongs to the glide. Left to the table, its own
                    // wheel handling would jump by its line step and end the
                    // glide, short of the edge when that was farther away.
                    cx.stop_propagation();
                }
            });
        },
    )
    .absolute()
    .size_full()
}

/// Gives the component library's own scrollbars (dialog bodies, popup lists)
/// the same look: always shown while the content overflows, the 4 px thumb in a
/// 10 px track that widens to 6 px at once under the pointer. Call after each
/// theme change, which rebuilds the library theme.
pub(in crate::frontend) fn style_library(dark: bool, cx: &mut App) {
    use gpui_kit::base::{ScrollbarMode, ScrollbarMotion, ScrollbarStyles, ScrollbarTheme};
    let normal = thumb_color(dark, false);
    let active = thumb_color(dark, true);
    let none = gpui_kit::transparent_black();
    gpui_kit::base::Theme::global_mut(cx).scrollbar = ScrollbarTheme::new()
        .with_mode(ScrollbarMode::Always)
        .with_motion(
            ScrollbarMotion::default()
                .with_enter(Duration::ZERO)
                .with_exit(Duration::ZERO)
                .with_expand(Duration::ZERO),
        )
        .with_styles(
            ScrollbarStyles::default()
                .track(|t| t.bg(none).border_color(none).width(px(SIZE)))
                .track_hover(|t| t.bg(none).border_color(none).width(px(SIZE)))
                .track_active(|t| t.bg(none).border_color(none).width(px(SIZE)))
                .thumb(|t| {
                    t.bg(normal)
                        .width(px(4.))
                        .inset(px(3.))
                        .radius(px(2.))
                        .min_length(px(MIN_THUMB))
                })
                .thumb_hover(|t| {
                    t.bg(active)
                        .width(px(6.))
                        .inset(px(2.))
                        .radius(px(3.))
                        .min_length(px(MIN_THUMB))
                })
                .thumb_active(|t| {
                    t.bg(active)
                        .width(px(6.))
                        .inset(px(2.))
                        .radius(px(3.))
                        .min_length(px(MIN_THUMB))
                }),
        );
}
