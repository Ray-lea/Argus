//! Text with elements set in its lines, as the interface's inline boxes: the
//! panel button in the overview tip and keycaps inside sentences.
//!
//! GPUI text has no inline boxes. An invisible word as wide as an element and
//! its margins holds the element's place, so the element moves to the next
//! line whole, at the same break opportunities as an inline box between
//! spaces, and is drawn over the word wherever the text put it.
use super::*;
use std::ops::Range;

/// How an element sits on the line it was laid out in.
#[derive(Clone, Copy)]
pub(super) enum Align {
    /// Centred in its line; lines are at least as tall as the element.
    Middle,
    /// Its baseline, `baseline` below its top, sits `drop` below the text's
    /// (`vertical-align: -drop`). A CSS line box grows to hold an element
    /// reaching below it; GPUI lines are all equally tall, so the text keeps
    /// that room below its last line, and on an earlier line the element
    /// stays inside its own.
    Baseline { baseline: Pixels, drop: Pixels },
}

pub(super) struct Part {
    marker: char,
    element: AnyElement,
    margin: Pixels,
    align: Align,
}

/// `element` in place of `marker`, with `margin` on either side.
pub(super) fn part(marker: char, element: impl IntoElement, margin: Pixels, align: Align) -> Part {
    Part {
        marker,
        element: element.into_any_element(),
        margin,
        align,
    }
}

/// `text` in the inherited text style, each part's marker replaced by its element.
pub(super) fn inline_text(text: impl Into<SharedString>, parts: Vec<Part>) -> InlineText {
    InlineText {
        text: text.into(),
        parts,
        body: None,
        layout: TextLayout::default(),
        slots: Vec::new(),
    }
}

pub(super) struct InlineText {
    text: SharedString,
    parts: Vec<Part>,
    body: Option<AnyElement>,
    layout: TextLayout,
    /// Where each part's word lies in the laid out text, and the part's size.
    slots: Vec<(Range<usize>, Size<Pixels>)>,
}

impl IntoElement for InlineText {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for InlineText {
    type RequestLayoutState = ();
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
        let style = window.text_style();
        let font = style.font();
        let font_size = style.font_size.to_pixels(window.rem_size());
        let mut line_height = window.pixel_snap(
            style
                .line_height
                .to_pixels(font_size.into(), window.rem_size()),
        );
        let sizes: Vec<Size<Pixels>> = self
            .parts
            .iter_mut()
            .map(|part| {
                part.element
                    .layout_as_root(AvailableSpace::min_size(), window, cx)
            })
            .collect();
        for (part, size) in self.parts.iter().zip(&sizes) {
            if matches!(part.align, Align::Middle) {
                line_height = line_height.max(window.pixel_snap(size.height));
            }
        }
        // The text with a word in place of each marker.
        let mut text = String::with_capacity(self.text.len());
        let mut slots = vec![None; self.parts.len()];
        for c in self.text.chars() {
            match self
                .parts
                .iter()
                .enumerate()
                .position(|(index, part)| part.marker == c && slots[index].is_none())
            {
                Some(index) => {
                    let width = sizes[index].width + self.parts[index].margin * 2.;
                    let word = room_word(width, font_size, &font, window);
                    slots[index] = Some(text.len()..text.len() + word.len());
                    text.push_str(&word);
                }
                None => text.push(c),
            }
        }
        // Parts whose marker the text lacks are not drawn.
        let present: Vec<usize> = (0..self.parts.len())
            .filter(|&index| slots[index].is_some())
            .collect();
        // Room below the last line for an element reaching below it.
        let mut overhang = px(0.);
        if present
            .iter()
            .any(|&index| matches!(self.parts[index].align, Align::Baseline { .. }))
        {
            let shaped = window.text_system().shape_line(
                SharedString::from(text.clone()),
                font_size,
                &[style.to_run(text.len())],
                None,
            );
            let text_baseline = (line_height - shaped.ascent - shaped.descent) / 2. + shaped.ascent;
            for &index in &present {
                if let Align::Baseline { baseline, drop } = self.parts[index].align {
                    overhang = overhang
                        .max(text_baseline + drop - baseline + sizes[index].height - line_height);
                }
            }
        }
        let highlights: Vec<(Range<usize>, HighlightStyle)> = present
            .iter()
            .filter_map(|&index| slots[index].clone())
            .map(|range| {
                (
                    range,
                    HighlightStyle {
                        fade_out: Some(1.),
                        ..Default::default()
                    },
                )
            })
            .collect();
        let sentence = StyledText::new(text).with_highlights(highlights);
        self.layout = sentence.layout().clone();
        self.slots = (0..self.parts.len())
            .map(|index| (slots[index].clone().unwrap_or(0..0), sizes[index]))
            .collect();
        let mut body = div()
            .line_height(line_height)
            .pb(overhang)
            .child(sentence)
            .into_any_element();
        let id = body.request_layout(window, cx);
        self.body = Some(body);
        (id, ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let Some(body) = self.body.as_mut() else {
            return;
        };
        body.prepaint(window, cx);
        let layout = &self.layout;
        let line_height = layout.line_height();
        let last = layout.position_for_index(layout.len());
        for (part, (range, size)) in self.parts.iter_mut().zip(&self.slots) {
            if range.is_empty() {
                continue;
            }
            // The word's end lies on the line holding the word, even where
            // a line starts after it.
            let (Some(at), Some(line)) = (
                layout.position_for_index(range.end),
                layout.line_layout_for_index(range.start),
            ) else {
                continue;
            };
            let line = &line.unwrapped_layout;
            let room = line.x_for_index(range.end) - line.x_for_index(range.start);
            let top = match part.align {
                Align::Middle => at.y + (line_height - size.height) / 2.,
                Align::Baseline { baseline, drop } => {
                    let text_baseline =
                        at.y + (line_height - line.ascent - line.descent) / 2. + line.ascent;
                    let top = text_baseline + drop - baseline;
                    if last.is_some_and(|last| last.y > at.y) {
                        top.min(at.y + line_height - size.height)
                    } else {
                        top
                    }
                }
            };
            let left = at.x - room / 2. - size.width / 2.;
            part.element
                .prepaint_at(window.pixel_snap_point(point(left, top)), window, cx);
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(body) = self.body.as_mut() {
            body.paint(window, cx);
        }
        for (part, (range, _)) in self.parts.iter_mut().zip(&self.slots) {
            if !range.is_empty() {
                part.element.paint(window, cx);
            }
        }
    }
}

/// A word of Latin letters about `width` wide in `font` at `size`: a line
/// never breaks inside it, as it never does inside an inline box.
fn room_word(width: Pixels, size: Pixels, font: &Font, window: &Window) -> String {
    thread_local! {
        static WORDS: RefCell<HashMap<(Font, u32, u32), String>> = RefCell::default();
    }
    let key = (
        font.clone(),
        f32::from(width).to_bits(),
        f32::from(size).to_bits(),
    );
    if let Some(word) = WORDS.with_borrow(|words| words.get(&key).cloned()) {
        return word;
    }
    // Letters of many widths, so that a few of them add up close to any width.
    const LETTERS: &str = "MWmwHNnhoeuaszrtfjil";
    let advances: Vec<(char, f32)> = LETTERS
        .chars()
        .map(|c| {
            let run = TextRun {
                len: c.len_utf8(),
                font: font.clone(),
                color: Hsla::default(),
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let width = window
                .text_system()
                .layout_line(&c.to_string(), size, &[run], None)
                .width;
            (c, f32::from(width))
        })
        .collect();
    // The widest letter repeated takes the bulk of a wide element; a few
    // letters of the others make up the rest.
    let (widest, widest_width) =
        advances.iter().copied().fold(
            (' ', 0.),
            |best, letter| if letter.1 > best.1 { letter } else { best },
        );
    let bulk = ((f32::from(width) - 2. * widest_width) / widest_width)
        .floor()
        .max(0.) as usize;
    let target = f32::from(width) - bulk as f32 * widest_width;
    let mut best = (f32::INFINITY, String::new());
    let mut consider = |letters: &[usize]| {
        let total: f32 = letters.iter().map(|&i| advances[i].1).sum();
        let error = (total - target).abs();
        if error + 0.01 < best.0 {
            best = (error, letters.iter().map(|&i| advances[i].0).collect());
        }
    };
    let count = advances.len();
    for a in 0..count {
        consider(&[a]);
        for b in a..count {
            consider(&[a, b]);
            for c in b..count {
                consider(&[a, b, c]);
                for d in c..count {
                    consider(&[a, b, c, d]);
                }
            }
        }
    }
    let word = std::iter::repeat_n(widest, bulk)
        .chain(best.1.chars())
        .collect::<String>();
    WORDS.with_borrow_mut(|words| words.insert(key, word.clone()));
    word
}
