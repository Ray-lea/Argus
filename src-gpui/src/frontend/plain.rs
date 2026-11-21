//! A `div` without interactivity: a style, children and nothing else.
//!
//! A `Div` carries all that an element may listen for (`Interactivity`): a
//! large value that each builder call moves, and that every phase visits for
//! hitboxes, element state, focus, hover and the inspector even where nothing
//! listens. Table rows build dozens of such boxes on every frame that touches
//! them (the name cell's padding, icon box, name, image name and marks).
//! `Plain` lays out, prepaints and paints as a `div` of the same style does
//! (`Div`'s three phases without the interactivity), so the boxes, clips and
//! fills are the same to the device pixel. It takes no listeners, no id and
//! no opacity (the window's opacity is GPUI's own to set): a box that needs
//! any of them is a `div`.
use gpui_kit::gpui::Refineable as _;
use gpui_kit::*;
use std::cell::RefCell;
use std::rc::Rc;

/// What a box reports with its bounds as it prepaints (`Prepainted`).
type Report = Box<dyn FnOnce(Bounds<Pixels>, &mut Window, &mut App)>;

pub(super) struct Plain {
    style: StyleRefinement,
    children: Vec<AnyElement>,
    /// Called with the box's bounds as it prepaints, inside its clip, as a
    /// child that reports where it was laid out would be.
    prepainted: Option<Report>,
}

pub(super) fn plain() -> Plain {
    Plain {
        style: StyleRefinement::default(),
        children: Vec::new(),
        prepainted: None,
    }
}

/// A plain box that reports where it was laid out as it prepaints, inside
/// its clip, as a child that reports its place would.
pub(super) trait Prepainted: Sized {
    fn on_prepainted(self, f: impl FnOnce(Bounds<Pixels>, &mut Window, &mut App) + 'static)
    -> Self;
}

impl Prepainted for Plain {
    fn on_prepainted(
        mut self,
        f: impl FnOnce(Bounds<Pixels>, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.prepainted = Some(Box::new(f));
        self
    }
}

impl Styled for Plain {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl ParentElement for Plain {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl IntoElement for Plain {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Plain {
    type RequestLayoutState = Style;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
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
        let mut style = Style::default();
        style.refine(&self.style);
        debug_assert!(style.opacity.is_none(), "a plain box takes no opacity");
        let layout_id = window.with_text_style(style.text_style().cloned(), |window| {
            let children: Vec<LayoutId> = self
                .children
                .iter_mut()
                .map(|child| child.request_layout(window, cx))
                .collect();
            window.request_layout(style.clone(), children, cx)
        });
        (layout_id, style)
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        style: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) {
        window.with_text_style(style.text_style().cloned(), |window| {
            window.with_content_mask(style.overflow_mask(bounds, window.rem_size()), |window| {
                if let Some(prepainted) = self.prepainted.take() {
                    prepainted(padding_box(bounds, style, window), window, cx);
                }
                if style.display == Display::None {
                    return;
                }
                // As `div` does: text that is not painted is not reported to
                // assistive technology.
                window.with_a11y_text_hidden(style.visibility == Visibility::Hidden, |window| {
                    for child in &mut self.children {
                        child.prepaint(window, cx);
                    }
                })
            })
        })
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        style: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        if style.visibility == Visibility::Hidden {
            return;
        }
        style.paint(bounds, window, cx, |window, cx| {
            window.with_text_style(style.text_style().cloned(), |window| {
                window.with_content_mask(style.overflow_mask(bounds, window.rem_size()), |window| {
                    if style.display == Display::None {
                        return;
                    }
                    for child in &mut self.children {
                        child.paint(window, cx);
                    }
                })
            })
        });
    }
}

/// Where a child pinned to the box's edges (`absolute().inset_0()`) is laid
/// out, as a box reports its place: inside its borders, each as wide as the
/// layout makes it (a border rounds to the device pixels, and is at least one
/// wide), from the box's edges, which lie on whole device pixels.
fn padding_box(bounds: Bounds<Pixels>, style: &Style, window: &Window) -> Bounds<Pixels> {
    let scale = window.scale_factor();
    let widths = style.border_widths.to_pixels(window.rem_size());
    // `round_stroke_to_device_pixel`: half toward zero, at least one.
    let stroke = |width: Pixels| {
        if width == px(0.) {
            px(0.)
        } else {
            let device = f32::from(width).max(0.) * scale;
            px((device - 0.5).ceil().max(1.) / scale)
        }
    };
    Bounds::from_corners(
        bounds.origin + point(stroke(widths.left), stroke(widths.top)),
        bounds.bottom_right() - point(stroke(widths.right), stroke(widths.bottom)),
    )
}

/// A plain box holding one line of text and nothing else, as one node: what
/// a `div` (or a [`Plain`]) without padding or border lays out, clips and
/// paints around a string child. The text is laid out in the box's content,
/// which a box without padding or border shares with it, and the box takes
/// the text's size where its style gives it none. The text measures, wraps
/// and truncates as GPUI's text element does (`TextLayout`).
pub(super) struct PlainText {
    style: StyleRefinement,
    text: SharedString,
    prepainted: Option<Report>,
}

pub(super) fn plain_text(text: impl Into<SharedString>) -> PlainText {
    PlainText {
        style: StyleRefinement::default(),
        text: text.into(),
        prepainted: None,
    }
}

impl Prepainted for PlainText {
    fn on_prepainted(
        mut self,
        f: impl FnOnce(Bounds<Pixels>, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.prepainted = Some(Box::new(f));
        self
    }
}

impl Styled for PlainText {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl IntoElement for PlainText {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

/// The text as laid out: its lines, as GPUI's `TextLayout` keeps them.
struct Measured {
    lines: Vec<WrappedLine>,
    line_height: Pixels,
    wrap_width: Option<Pixels>,
    truncate_width: Option<Pixels>,
    size: Size<Pixels>,
    baseline: Option<Pixels>,
}

impl Measured {
    /// `TextLayoutInner::cached_size`: the size measured before, if it holds
    /// for these constraints.
    fn cached_size(
        &self,
        wrap_width: Option<Pixels>,
        truncate_width: Option<Pixels>,
        line_clamp: Option<usize>,
    ) -> Option<Size<Pixels>> {
        let size = self.size;
        let same_constraints =
            wrap_width == self.wrap_width && truncate_width == self.truncate_width;
        let fits_unwrapped = line_clamp.is_none()
            && self.truncate_width.is_none()
            && self
                .lines
                .iter()
                .all(|line| line.wrap_boundaries.is_empty())
            && wrap_width.is_none_or(|width| width >= size.width)
            && truncate_width.is_none_or(|width| width >= size.width);
        (same_constraints || fits_unwrapped).then_some(size)
    }
}

pub(super) struct PlainTextLayout {
    style: Style,
    measured: Rc<RefCell<Option<Measured>>>,
}

impl Element for PlainText {
    type RequestLayoutState = PlainTextLayout;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        _: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.refine(&self.style);
        debug_assert!(style.opacity.is_none(), "a plain box takes no opacity");
        debug_assert!(
            style.padding == Edges::default() && style.border_widths == Edges::default(),
            "plain text shares its box with the text: no padding or border"
        );
        let measured = Rc::new(RefCell::new(None));
        let layout_id = window.with_text_style(style.text_style().cloned(), |window| {
            measure_text(self.text.clone(), style.clone(), measured.clone(), window)
        });
        (layout_id, PlainTextLayout { style, measured })
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let style = &layout.style;
        // A text element reports its text to assistive technology as it
        // prepaints (`Window::report_a11y_text`), unless it is not painted.
        if style.visibility != Visibility::Hidden && style.display != Display::None {
            window.report_a11y_text(&self.text, bounds);
        }
        window.with_text_style(style.text_style().cloned(), |window| {
            window.with_content_mask(style.overflow_mask(bounds, window.rem_size()), |window| {
                if let Some(prepainted) = self.prepainted.take() {
                    prepainted(bounds, window, cx);
                }
            })
        })
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let style = &layout.style;
        if style.visibility == Visibility::Hidden {
            return;
        }
        let measured = layout.measured.borrow();
        style.paint(bounds, window, cx, |window, cx| {
            window.with_text_style(style.text_style().cloned(), |window| {
                window.with_content_mask(style.overflow_mask(bounds, window.rem_size()), |window| {
                    let Some(measured) =
                        measured.as_ref().filter(|_| style.display != Display::None)
                    else {
                        return;
                    };
                    // `TextLayout::paint_lines`; the runs have no backgrounds.
                    let align = window.text_style().text_align;
                    let mut origin = bounds.origin;
                    for line in &measured.lines {
                        line.paint(
                            origin,
                            measured.line_height,
                            align,
                            Some(bounds),
                            window,
                            cx,
                        )
                        .ok();
                        origin.y += line.size(measured.line_height).height;
                    }
                })
            })
        });
    }
}

/// Requests the node of `text` in `style`, measured as `TextLayout::layout`
/// measures a text element, in the text style of the box (pushed).
fn measure_text(
    text: SharedString,
    style: Style,
    measured: Rc<RefCell<Option<Measured>>>,
    window: &mut Window,
) -> LayoutId {
    let text_style = window.text_style();
    let font_size = text_style.font_size.to_pixels(window.rem_size());
    let line_height = window.pixel_snap(
        text_style
            .line_height
            .to_pixels(font_size.into(), window.rem_size()),
    );
    let runs = vec![text_style.to_run(text.len())];
    window.request_measured_layout_with_baseline(
        style,
        move |known_dimensions, available_space, window, cx| {
            let wrap_width = if text_style.white_space == WhiteSpace::Normal {
                known_dimensions.width.or(match available_space.width {
                    AvailableSpace::Definite(x) => Some(x),
                    _ => None,
                })
            } else {
                None
            };
            let (truncate_width, truncation_affix, truncate_from) =
                if let Some(text_overflow) = text_style.text_overflow.clone() {
                    let width = known_dimensions.width.or(match available_space.width {
                        AvailableSpace::Definite(x) => match text_style.line_clamp {
                            Some(max_lines) => Some(x * max_lines),
                            None => Some(x),
                        },
                        _ => None,
                    });
                    match text_overflow {
                        TextOverflow::Truncate(s) => (width, s, TruncateFrom::End),
                        TextOverflow::TruncateStart(s) => (width, s, TruncateFrom::Start),
                        TextOverflow::TruncateMiddle(s) => (width, s, TruncateFrom::Middle),
                    }
                } else {
                    (None, SharedString::default(), TruncateFrom::End)
                };
            if let Some(cached) = measured.borrow().as_ref()
                && let Some(size) =
                    cached.cached_size(wrap_width, truncate_width, text_style.line_clamp)
            {
                return (size, cached.baseline);
            }
            let (text, runs) = if let Some(truncate_width) = truncate_width {
                if let Some(max_lines) = text_style.line_clamp
                    && let Some(wrap_width) = wrap_width
                {
                    cx.text_system()
                        .line_wrapper(text_style.font(), font_size)
                        .truncate_wrapped_line(
                            text.clone(),
                            wrap_width,
                            max_lines,
                            &truncation_affix,
                            &runs,
                            truncate_from,
                        )
                } else if let Ok(unclipped) =
                    window
                        .text_system()
                        .shape_text(text.clone(), font_size, &runs, None, None)
                    && unclipped
                        .iter()
                        .all(|line| line.size(line_height).width <= truncate_width)
                {
                    // As GPUI's text: what fits as shaped is not cut short.
                    (text.clone(), std::borrow::Cow::Borrowed(&*runs))
                } else {
                    cx.text_system()
                        .line_wrapper(text_style.font(), font_size)
                        .truncate_line(
                            text.clone(),
                            truncate_width,
                            &truncation_affix,
                            &runs,
                            truncate_from,
                        )
                }
            } else {
                (text.clone(), std::borrow::Cow::Borrowed(&*runs))
            };
            let Ok(lines) = window.text_system().shape_text(
                text,
                font_size,
                &runs,
                wrap_width,
                text_style.line_clamp,
            ) else {
                measured.replace(Some(Measured {
                    lines: Vec::new(),
                    line_height,
                    wrap_width,
                    truncate_width,
                    size: Size::default(),
                    baseline: None,
                }));
                return (Size::default(), None);
            };
            let mut size: Size<Pixels> = Size::default();
            for line in &lines {
                let line_size = line.size(line_height);
                size.height += line_size.height;
                size.width = size.width.max(line_size.width).ceil();
            }
            let baseline =
                first_baseline(&lines, line_height, &text_style.font(), font_size, window);
            measured.replace(Some(Measured {
                lines: lines.into_iter().collect(),
                line_height,
                wrap_width,
                truncate_width,
                size,
                baseline,
            }));
            (size, baseline)
        },
    )
}

/// GPUI's `first_baseline`: the glyphs' ascent and descent centred in the
/// line height, or the font's for an empty line.
fn first_baseline(
    lines: &[WrappedLine],
    line_height: Pixels,
    font: &Font,
    font_size: Pixels,
    window: &Window,
) -> Option<Pixels> {
    let layout = &lines.first()?.unwrapped_layout;
    let (ascent, descent) = if layout.ascent > Pixels::ZERO || layout.descent > Pixels::ZERO {
        (layout.ascent, layout.descent)
    } else {
        let text_system = window.text_system();
        let font_id = text_system.resolve_font(font);
        (
            text_system.ascent(font_id, font_size),
            text_system.descent(font_id, font_size).abs(),
        )
    };
    Some((line_height - ascent - descent) / 2. + ascent)
}
