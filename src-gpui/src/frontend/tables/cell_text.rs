//! A table cell that shows one line of text, drawn by one element.
//!
//! Every frame that touches a table lays out its visible cells again. A cell
//! of text as elements is a flex `div` with the cell's padding, background
//! and text style, holding the text, with a `truncate()` wrapper between them
//! for values cut short with an ellipsis: two or three layout nodes and a
//! text measurement in every cell. This element lays out as that `div` (it
//! fills the cell) and puts the text where those nodes would: the same device
//! pixels, the same alignment within the text's own box, the same clips, and
//! the same area for the cell's hint. Cells with marks or a switch keep their
//! elements (`td_content`).
use super::*;

/// What the cell's `div` and its text showed.
pub(super) struct CellText {
    text: SharedString,
    /// The `div`'s text style over the one it inherits.
    style: TextStyleRefinement,
    background: Option<Hsla>,
    /// The `div`'s padding on either side.
    padding: Pixels,
    /// The text at the end of the cell (`justify_end`).
    end: bool,
    /// The text in a `truncate()` wrapper: cut short with an ellipsis when
    /// it does not fit, and clipped to the wrapper.
    truncate: bool,
    hint: Option<Hint>,
}

/// The cell's `title`, which the row under the pointer reports (`tips`).
struct Hint {
    tips: tips::Tips,
    id: u64,
    text: SharedString,
    /// Over the whole cell rather than the text's own box.
    whole: bool,
}

pub(super) fn cell_text(text: SharedString, font_size: Pixels, padding: Pixels) -> CellText {
    CellText {
        text,
        style: TextStyleRefinement {
            font_size: Some(font_size.into()),
            line_height: Some(theme::line_height()),
            ..Default::default()
        },
        background: None,
        padding,
        end: false,
        truncate: false,
        hint: None,
    }
}

impl CellText {
    pub(super) fn color(mut self, color: Hsla) -> Self {
        self.style.color = Some(color);
        self
    }
    pub(super) fn font_family(mut self, family: SharedString) -> Self {
        self.style.font_family = Some(family);
        self
    }
    pub(super) fn background(mut self, background: Hsla) -> Self {
        self.background = Some(background);
        self
    }
    pub(super) fn end(mut self, end: bool) -> Self {
        self.end = end;
        self
    }
    pub(super) fn truncate(mut self) -> Self {
        self.truncate = true;
        self
    }
    /// The alignment the text would inherit from the cell's box, for a cell
    /// that is drawn without one (`body`).
    pub(super) fn align(&mut self, align: TextAlign) {
        self.style.text_align = Some(align);
    }
    /// The cell's `title` over the whole cell (`whole`) or over its text.
    pub(super) fn hint(
        mut self,
        tips: &tips::Tips,
        id: u64,
        text: impl Into<SharedString>,
        whole: bool,
    ) -> Self {
        self.hint = Some(Hint {
            tips: tips.clone(),
            id,
            text: text.into(),
            whole,
        });
        self
    }
}

pub(super) struct Placed {
    line: WrappedLine,
    line_height: Pixels,
    /// The text's own box: its line is aligned within it.
    bounds: Bounds<Pixels>,
    align: TextAlign,
}

impl IntoElement for CellText {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for CellText {
    type RequestLayoutState = ();
    type PrepaintState = Option<Placed>;

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
        // The `div`'s box: it fills the cell. Its padding and children only
        // placed the text inside it, which `prepaint` does.
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, None, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        // The box relative to the cell's layout, which `layout_bounds` snapped.
        let top = (f32::from(bounds.origin.y - window.element_offset().y) * window.scale_factor())
            .round();
        self.settle(bounds, top, window, cx)
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        placed: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.draw(bounds, placed.as_ref(), window, cx);
    }
}

impl CellText {
    /// Places the text in a cell at `bounds` and reports the cell's hint:
    /// what the element does as it prepaints, also for a row that draws its
    /// cells itself (`body`). `top` is how far down the cell lies in the
    /// layout it is part of, in device pixels: none for a cell laid out on
    /// its own.
    pub(super) fn settle(
        &mut self,
        bounds: Bounds<Pixels>,
        top: f32,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Placed> {
        let placed = window.with_text_style(Some(self.style.clone()), |window| {
            self.place(bounds, top, window, cx)
        });
        // Where `Tips::target` on the `div`, or on the `truncate()` wrapper,
        // would report the part.
        if let Some(hint) = self.hint.take() {
            let area = if hint.whole {
                Some(bounds)
            } else {
                placed.as_ref().map(|placed| placed.bounds)
            };
            if let Some(area) = area {
                hint.tips.report(hint.id, area, hint.text, window, cx);
            }
        }
        placed
    }

    /// Paints the cell at `bounds` with the text where `settle` placed it.
    pub(super) fn draw(
        &self,
        bounds: Bounds<Pixels>,
        placed: Option<&Placed>,
        window: &mut Window,
        cx: &mut App,
    ) {
        // As `Style::paint` fills a `div`: a square quad, its border the
        // background's colour, transparent; none for a transparent fill
        // (the heat of an idle reading).
        if let Some(background) = self.background.filter(|fill| !fill.is_transparent()) {
            window.paint_quad(quad(
                bounds,
                Corners::default(),
                background,
                Edges::default(),
                background.opacity(0.),
                BorderStyle::default(),
            ));
        }
        let Some(placed) = placed else {
            return;
        };
        // The `div` hides what overflows it, and so does the `truncate()`
        // wrapper, which is as large as the text's box.
        let clip = if self.truncate {
            bounds.intersect(&placed.bounds)
        } else {
            bounds
        };
        window.with_content_mask(Some(ContentMask { bounds: clip }), |window| {
            placed
                .line
                .paint(
                    placed.bounds.origin,
                    placed.line_height,
                    placed.align,
                    Some(placed.bounds),
                    window,
                    cx,
                )
                .ok();
        });
    }
}

impl CellText {
    /// Where the `div`'s flex layout and the device-pixel snapping put the
    /// text's box (`gpui::taffy`), and the line it shows. In device pixels:
    /// lengths authored in the style round half toward zero, a measured text
    /// box rounds up, and each box's edges round half toward zero where they
    /// land relative to the cell, which starts on a whole device pixel.
    fn place(
        &self,
        bounds: Bounds<Pixels>,
        top: f32,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Placed> {
        let text_style = window.text_style();
        let font_size = text_style.font_size.to_pixels(window.rem_size());
        let line_height = window.pixel_snap(
            text_style
                .line_height
                .to_pixels(font_size.into(), window.rem_size()),
        );
        let runs = vec![text_style.to_run(self.text.len())];
        let scale = window.scale_factor();
        let device = |value: Pixels| f32::from(value) * scale;
        let width = device(bounds.size.width).round();
        let height = device(bounds.size.height).round();
        let padding = round_half_toward_zero(device(self.padding));

        let line = window
            .text_system()
            .shape_text(self.text.clone(), font_size, &runs, None, None)
            .ok()?
            .into_iter()
            .next()?;
        let shaped_width = line.width();
        // The text measures its line rounded up to a whole pixel; the layout
        // then rounds that up to a whole device pixel.
        let text_width = (f32::from(shaped_width).ceil() * scale).ceil();
        let text_height = device(line_height).ceil();
        let (box_width, line) = if self.truncate {
            // The wrapper shrinks to the room the padding leaves, and the
            // text, given the wrapper's width, is cut short when its line does
            // not fit in it.
            let room = (width - 2. * padding).max(0.);
            let box_width = text_width.min(room);
            let truncate_width = px(box_width / scale);
            if shaped_width > truncate_width {
                let (text, runs) = cx
                    .text_system()
                    .line_wrapper(text_style.font(), font_size)
                    .truncate_line(
                        self.text.clone(),
                        truncate_width,
                        "…",
                        &runs,
                        TruncateFrom::End,
                    );
                let line = window
                    .text_system()
                    .shape_text(text, font_size, &runs, None, None)
                    .ok()?
                    .into_iter()
                    .next()?;
                (box_width, line)
            } else {
                (box_width, line)
            }
        } else {
            (text_width, line)
        };
        let x = if self.end {
            width - padding - box_width
        } else {
            padding
        };
        // Centred across the row, then snapped where it lands.
        let y = round_half_toward_zero(top + (height - text_height) / 2.) - top;
        Some(Placed {
            line,
            line_height,
            bounds: Bounds::new(
                bounds.origin + point(px(x / scale), px(y / scale)),
                size(px(box_width / scale), px(text_height / scale)),
            ),
            align: text_style.text_align,
        })
    }
}

/// `gpui`'s rounding of layout edges to device pixels.
pub(super) fn round_half_toward_zero(value: f32) -> f32 {
    (value.abs() - 0.5).ceil().copysign(value)
}
