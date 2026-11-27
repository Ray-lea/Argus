//! How the desktop monitor draws its readings, in its window and in the
//! style previews on the components page: a panel of groups, one device's
//! readings to a group, as a list, curves or gauges; or a strip along a
//! screen edge, wrapping between groups where the display is too narrow.
use super::*;
use crate::settings::{DeskGrouping, DeskLayout, DeskStyle};
use gpui_kit::component::{ActiveTheme, ThemeMode};
use std::cell::{Cell, RefCell};
use std::ops::Range;
use std::rc::Rc;

/// The panel's width in each style, logical pixels.
pub(in crate::frontend) fn panel_width(style: DeskStyle) -> f32 {
    match style {
        DeskStyle::List => 264.,
        DeskStyle::Curves => 280.,
        DeskStyle::Gauges => 320.,
    }
}

/// How many samples a curve shows.
pub(in crate::frontend) const CURVE_SAMPLES: usize = 60;

/// A reading's recent values for its curve, 0–1 and oldest first: one
/// series, or the two of a pair of rates on one scale; none without any.
pub(in crate::frontend) type Curve = Vec<Vec<f32>>;

/// What the monitor draws: its readings, their curves (none unless it
/// shows curves) and how.
pub(in crate::frontend) struct Drawn<'a> {
    pub gauges: &'a [Gauge],
    pub curves: &'a [Curve],
    pub style: DeskStyle,
    pub grouping: DeskGrouping,
}

/// The kind of device a metric reads, for grouping; time since Windows
/// started is a group of its own.
fn family(metric: Metric) -> u8 {
    match metric {
        Metric::Cpu | Metric::CpuTemperature | Metric::CpuClock | Metric::CpuPower => 0,
        Metric::Memory | Metric::MemoryUsed => 1,
        Metric::Battery | Metric::BatteryPower => 5,
        Metric::Uptime => 6,
        metric => match device_kind(metric) {
            Some("gpu") => 2,
            Some("disk") => 3,
            _ => 4,
        },
    }
}

/// The readings in turn that are of one device, as ranges of `gauges`:
/// the groups the monitor shows.
pub(in crate::frontend) fn groups(gauges: &[Gauge]) -> Vec<Range<usize>> {
    let mut groups: Vec<Range<usize>> = Vec::new();
    for (index, gauge) in gauges.iter().enumerate() {
        let reading = &gauge.reading;
        let joins = index > 0 && {
            let before = &gauges[index - 1].reading;
            family(reading.metric) != family(Metric::Uptime)
                && family(reading.metric) == family(before.metric)
                && reading.device == before.device
        };
        match groups.last_mut() {
            Some(group) if joins => group.end = index + 1,
            _ => groups.push(index..index + 1),
        }
    }
    groups
}

/// `text` as runs of figures and of units, the spaces between them left
/// out: "3.94 GHz" is "3.94" and "GHz"; a space between digits (a
/// thousands separator) and one inside a unit ("% TDP") stay.
pub(in crate::frontend) fn pieces(text: &str) -> Vec<(String, bool)> {
    let chars = text.chars().collect::<Vec<_>>();
    let mut pieces: Vec<(String, bool)> = Vec::new();
    for (index, &c) in chars.iter().enumerate() {
        let digit = |at: Option<&char>| at.is_some_and(char::is_ascii_digit);
        if c.is_whitespace() {
            let next = chars.get(index + 1);
            match pieces.last_mut() {
                Some((piece, false))
                    if digit(index.checked_sub(1).map(|i| &chars[i])) && digit(next) =>
                {
                    piece.push(c)
                }
                Some((piece, true)) if next.is_some_and(|n| !n.is_whitespace()) && !digit(next) => {
                    piece.push(c)
                }
                _ => {}
            }
            continue;
        }
        let unit =
            !(c.is_ascii_digit() || matches!(c, '.' | ',' | ':' | '+' | '-' | '−' | '–' | '—'));
        match pieces.last_mut() {
            Some((piece, kind)) if *kind == unit => piece.push(c),
            _ => pieces.push((c.to_string(), unit)),
        }
    }
    pieces
}

/// How a value is drawn: its figures and its units, in pixels at the
/// theme's base font size; figures in the display face when `display`.
#[derive(Clone, Copy)]
struct Face {
    figure: f32,
    unit: f32,
    display: bool,
}

/// A group's first reading.
const BIG: Face = Face {
    figure: 20.,
    unit: 12.5,
    display: true,
};
/// Rates beside a group's name.
const RATE: Face = Face {
    figure: 16.,
    unit: 11.5,
    display: true,
};
/// Rates in a tile, and values in the strip.
const TALL: Face = Face {
    figure: 17.,
    unit: 12.,
    display: true,
};
/// The other readings of a group.
const SMALL: Face = Face {
    figure: 14.,
    unit: 11.5,
    display: false,
};
/// The other readings beside a curve.
const KEYED: Face = Face {
    figure: 13.5,
    unit: 11.5,
    display: false,
};

/// The width of `text` at `size`, in the display face when `display`.
fn text_width(
    text: &str,
    size: f32,
    weight: FontWeight,
    display: bool,
    window: &Window,
    cx: &App,
) -> f32 {
    let mut run = theme::text_run(text, weight, cx);
    if display {
        run.font.family = theme::display_font(cx);
    }
    f32::from(
        window
            .text_system()
            .layout_line(text, theme::font(size), &[run], None)
            .width,
    )
    .ceil()
}

/// The room between pieces of a value: before a unit, and before a figure
/// after one ("5 天 14 小时").
fn spacing(unit: bool) -> f32 {
    if unit { 2. } else { 4. }
}

/// How wide `text` draws as a value in `size`.
fn value_width(text: &str, size: Face, window: &Window, cx: &App) -> f32 {
    pieces(text)
        .iter()
        .enumerate()
        .map(|(index, (piece, unit))| {
            let gap = if index == 0 { 0. } else { spacing(*unit) };
            gap + if *unit {
                text_width(piece, size.unit, FontWeight::MEDIUM, false, window, cx)
            } else {
                text_width(
                    piece,
                    size.figure,
                    FontWeight::SEMIBOLD,
                    size.display,
                    window,
                    cx,
                )
            }
        })
        .sum()
}

/// A value: figures in `color`, units smaller in `unit` (the same colour
/// when the reading is high), on one baseline.
fn value(text: &str, size: Face, color: Hsla, unit: Hsla, cx: &App) -> Div {
    let display = size.display.then(|| theme::display_font(cx));
    div()
        .flex()
        .flex_none()
        .items_baseline()
        .whitespace_nowrap()
        .children(
            pieces(text)
                .into_iter()
                .enumerate()
                .map(|(index, (piece, is_unit))| {
                    div()
                        .when(index > 0, |piece| piece.ml(px(spacing(is_unit))))
                        .text_size(theme::font(if is_unit { size.unit } else { size.figure }))
                        .font_weight(if is_unit {
                            FontWeight::MEDIUM
                        } else {
                            FontWeight::SEMIBOLD
                        })
                        .text_color(if is_unit { unit } else { color })
                        .when_some(display.clone().filter(|_| !is_unit), |piece, face| {
                            piece.font_family(face)
                        })
                        .child(piece)
                }),
        )
}

/// A gauge's single value, amber when high.
fn reading(gauge: &Gauge, size: Face, p: &Palette, cx: &App) -> Div {
    let text = gauge.parts.first().map_or("", |part| part.text.as_str());
    let (color, unit) = if gauge.high {
        (p.heat, p.heat)
    } else {
        (p.ink, p.muted)
    };
    value(text, size, color, unit, cx)
}

/// The width a gauge's single value keeps: its widest text's.
fn reserve(gauge: &Gauge, size: Face, window: &Window, cx: &App) -> f32 {
    gauge.parts.first().map_or(0., |part| {
        value_width(&part.widest, size, window, cx).max(value_width(&part.text, size, window, cx))
    })
}

/// The colour of a gauge's device, or amber when it is high.
fn tint(gauge: &Gauge, p: &Palette) -> Hsla {
    if gauge.high {
        p.heat
    } else {
        metric_color(gauge.reading.metric, *p)
    }
}

/// What a reading is called beside its group's first: the metric's option
/// on the components page ("温度", "用量").
fn option(gauge: &Gauge) -> String {
    label(&format!(
        "components.metric.{}.option",
        metric_key(gauge.reading.metric)
    ))
}

/// A reading's name, truncated.
fn name(text: impl Into<SharedString>, size: f32, p: &Palette) -> Div {
    div()
        .min_w_0()
        .truncate()
        .text_size(theme::font(size))
        .text_color(p.muted)
        .child(text.into())
}

/// A bar of `height` px filled to the gauge's fraction.
fn bar(gauge: &Gauge, height: f32, p: &Palette) -> Div {
    div()
        .flex_1()
        .min_w(px(28.))
        .h(px(height))
        .rounded(px(height / 2.))
        .bg(p.stroke_strong)
        .overflow_hidden()
        .child(
            div()
                .h_full()
                .w(relative(gauge.fraction.unwrap_or(0.) as f32))
                .rounded(px(height / 2.))
                .bg(tint(gauge, p)),
        )
}

/// A rate of a pair: its mark in the device's colour, then its value;
/// `width` keeps its widest text's room.
fn rate(gauge: &Gauge, part: &Part, size: Face, p: &Palette, cx: &App) -> Div {
    div()
        .flex()
        .flex_none()
        .items_baseline()
        .justify_end()
        .gap(px(4.))
        .child(
            div()
                .text_size(theme::font(13.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(metric_color(gauge.reading.metric, *p))
                .child(part.mark.clone()),
        )
        .child(value(&part.text, size, p.ink, p.muted, cx))
}

/// The room a rate of a pair keeps.
fn rate_width(part: &Part, size: Face, window: &Window, cx: &App) -> f32 {
    text_width(&part.mark, 13., FontWeight::SEMIBOLD, false, window, cx)
        + 4.
        + value_width(&part.widest, size, window, cx).max(value_width(&part.text, size, window, cx))
}

/// The other readings of a group, `columns` to a row: a name over a value.
fn stats(gauges: &[&Gauge], columns: u16, p: &Palette, cx: &App) -> Div {
    div()
        .grid()
        .grid_cols(columns)
        .gap(px(6.))
        .children(gauges.iter().map(|gauge| {
            let shown = match gauge.parts.as_slice() {
                [_] => reading(gauge, SMALL, p, cx).into_any_element(),
                parts => div()
                    .flex()
                    .gap(px(6.))
                    .children(parts.iter().map(|part| rate(gauge, part, SMALL, p, cx)))
                    .into_any_element(),
            };
            div()
                .flex()
                .flex_col()
                .min_w_0()
                .child(name(option(gauge), 12., p).line_height(theme::font(16.)))
                .child(div().h(theme::font(19.)).flex().items_center().child(shown))
        }))
}

/// The panel's floor under its cards and tiles: a step darker than them.
fn floor(cx: &App) -> Hsla {
    theme::token("monitor-floor", cx.theme().mode == ThemeMode::Dark)
}

/// What the monitor's window shows under its content.
pub(in crate::frontend) fn ground(
    layout: DeskLayout,
    style: DeskStyle,
    grouping: DeskGrouping,
    p: &Palette,
    cx: &App,
) -> Hsla {
    match (layout, style, grouping) {
        (DeskLayout::Panel, DeskStyle::Gauges, _) | (DeskLayout::Panel, _, DeskGrouping::Cards) => {
            floor(cx)
        }
        _ => widgets::solid(p.flyout),
    }
}

/// The panel: its groups in `style`, set apart by `grouping`; `width`
/// pixels wide, or as wide as it is given (a preview).
pub(in crate::frontend) fn panel(
    shown: &Drawn,
    width: Option<f32>,
    p: &Palette,
    window: &Window,
    cx: &App,
) -> Div {
    let ranges = groups(shown.gauges);
    let sized = |panel: Div| match width {
        Some(width) => panel.w(px(width)),
        None => panel.w_full(),
    };
    if shown.style == DeskStyle::Gauges {
        let count = ranges.len();
        return sized(div())
            .flex()
            .flex_wrap()
            .gap(px(6.))
            .p(px(6.))
            .children(ranges.into_iter().enumerate().map(|(index, range)| {
                // A tile left alone in the last row takes the whole row.
                let wide = index + 1 == count && count % 2 == 1;
                tile(&shown.gauges[range], wide, p, cx)
            }));
    }
    // The room inside a group, for the names before bars.
    let inner = width.unwrap_or(panel_width(shown.style)) - 32.;
    let grouping = shown.grouping;
    let container = match grouping {
        DeskGrouping::Cards => div().p(px(6.)).gap(px(6.)),
        DeskGrouping::Stripes => div()
            .pt(px(10.))
            .pb(px(10.))
            .pl(px(10.))
            .pr(px(12.))
            .gap(px(12.)),
        DeskGrouping::Rules => div().px(px(12.)),
    };
    sized(container)
        .flex()
        .flex_col()
        .children(ranges.into_iter().enumerate().map(|(index, range)| {
            let gauges = &shown.gauges[range.clone()];
            let content = match shown.style {
                DeskStyle::Curves => {
                    let curve = shown.curves.get(range.start).cloned().unwrap_or_default();
                    curves_group(gauges, curve, grouping, p, cx)
                }
                _ => list_group(gauges, inner, p, window, cx),
            };
            let curves = shown.style == DeskStyle::Curves;
            let group = div().relative().flex_none().overflow_hidden();
            match grouping {
                DeskGrouping::Cards => group
                    .bg(p.layer_strong)
                    .border_1()
                    .border_color(p.stroke)
                    .rounded(px(6.))
                    .when(!curves, |group| group.px(px(10.)).py(px(8.))),
                DeskGrouping::Stripes => group.pl(px(11.)).child(
                    div()
                        .absolute()
                        .left_0()
                        .top(px(2.))
                        .bottom(px(2.))
                        .w(px(3.))
                        .rounded(px(1.5))
                        .bg(metric_color(gauges[0].reading.metric, *p)),
                ),
                DeskGrouping::Rules => group
                    .when(!curves, |group| group.py(px(8.)))
                    .when(index > 0, |group| {
                        group.border_t_1().border_color(p.stroke_strong)
                    }),
            }
            .child(content)
        }))
}

/// A group of the list: its first reading's name, bar and value; a row
/// with a bar for each other reading that has a scale, the bars starting
/// and ending together; the rest in three columns under them.
fn list_group(gauges: &[Gauge], inner: f32, p: &Palette, window: &Window, cx: &App) -> Div {
    let first = &gauges[0];
    let barred = |gauge: &&Gauge| gauge.bar && gauge.parts.len() == 1;
    let rows = gauges[1..].iter().filter(barred).collect::<Vec<_>>();
    let rest = gauges[1..]
        .iter()
        .filter(|gauge| !barred(gauge))
        .collect::<Vec<_>>();
    let mut group = div().flex().flex_col();
    if let [one, two] = first.parts.as_slice() {
        group = group.child(
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .h(theme::font(28.))
                .child(name(first.name.clone(), 13., p).flex_1())
                .children([one, two].map(|part| {
                    rate(first, part, RATE, p, cx).min_w(px(rate_width(part, RATE, window, cx)))
                })),
        );
    }
    let single = first.parts.len() == 1;
    // The names before bars share a column, and the values another of at
    // least 64 px, so that the panel's bars end together.
    let names = rows
        .iter()
        .map(|gauge| text_width(&option(gauge), 12., FontWeight::NORMAL, false, window, cx))
        .chain(single.then(|| text_width(&first.name, 13., FontWeight::NORMAL, false, window, cx)))
        .fold(0., f32::max)
        .min(inner * 0.5);
    let values = rows
        .iter()
        .map(|gauge| reserve(gauge, SMALL, window, cx))
        .chain(single.then(|| reserve(first, BIG, window, cx)))
        .fold(64., f32::max);
    let line = |label: Div, gauge: &Gauge, height: f32, value: Div, bar_height: f32| {
        div()
            .flex()
            .items_center()
            .gap(px(10.))
            .h(theme::font(height))
            .child(label.w(px(names)).flex_none())
            .child(if gauge.bar {
                bar(gauge, bar_height, p)
            } else {
                div().flex_1()
            })
            .child(
                div()
                    .w(px(values))
                    .flex_none()
                    .flex()
                    .justify_end()
                    .child(value),
            )
    };
    if single {
        group = group.child(line(
            name(first.name.clone(), 13., p),
            first,
            28.,
            reading(first, BIG, p, cx),
            6.,
        ));
    }
    for gauge in rows {
        group = group.child(line(
            name(option(gauge), 12., p),
            gauge,
            22.,
            reading(gauge, SMALL, p, cx),
            5.,
        ));
    }
    group.when(!rest.is_empty(), |group| {
        group.child(stats(&rest, 3, p, cx).mt(px(4.)).mb(px(2.)))
    })
}

/// A group of the curves: its names and values on the left, its first
/// reading's last minute on the right over the group's whole height, from
/// 0 at its bottom to full scale at its top.
fn curves_group(
    gauges: &[Gauge],
    curve: Curve,
    grouping: DeskGrouping,
    p: &Palette,
    cx: &App,
) -> Div {
    let first = &gauges[0];
    let head = match first.parts.as_slice() {
        [one, two] => div()
            .flex()
            .items_start()
            .gap(px(8.))
            .child(
                name(first.name.clone(), 13., p)
                    .flex_1()
                    .h(theme::font(28.))
                    .flex()
                    .items_center(),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_end()
                    .child(
                        div()
                            .h(theme::font(28.))
                            .flex()
                            .items_center()
                            .child(rate(first, one, RATE, p, cx)),
                    )
                    .child(
                        div()
                            .h(theme::font(22.))
                            .flex()
                            .items_center()
                            .child(rate(first, two, RATE, p, cx)),
                    ),
            ),
        _ => div()
            .flex()
            .items_center()
            .gap(px(8.))
            .h(theme::font(28.))
            .child(name(first.name.clone(), 13., p).flex_1())
            .child(reading(first, BIG, p, cx)),
    };
    let text = div()
        .flex()
        .flex_col()
        .justify_center()
        .min_w_0()
        .child(head)
        .children(gauges[1..].iter().map(|gauge| {
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(8.))
                .h(theme::font(19.))
                .child(name(option(gauge), 12., p))
                .child(match gauge.parts.as_slice() {
                    [_] => reading(gauge, KEYED, p, cx),
                    parts => div()
                        .flex()
                        .gap(px(6.))
                        .children(parts.iter().map(|part| rate(gauge, part, KEYED, p, cx))),
                })
        }));
    let text = match grouping {
        DeskGrouping::Cards => text.px(px(10.)).py(px(8.)),
        DeskGrouping::Stripes => text.pr(px(10.)),
        DeskGrouping::Rules => text.pr(px(10.)).py(px(8.)),
    };
    // A reading kept nowhere over time (uptime) has no curve.
    if first.trend.is_none() {
        return div().flex().child(text.flex_1());
    }
    let color = metric_color(first.reading.metric, *p);
    let grid = p.stroke;
    div()
        .flex()
        .min_h(px(52.))
        .child(text.w(px(148.)).flex_none())
        .child(
            div()
                .relative()
                .flex_1()
                .min_w_0()
                .border_l_1()
                .border_color(grid)
                .child(plot(curve, color, grid).absolute().inset_0()),
        )
}

/// Paints `curve` over its bounds: scale lines at a quarter, half and
/// three quarters; the first series as a line over a fading area, the
/// second (what a pair sends or writes) as a fainter line.
fn plot(curve: Curve, color: Hsla, grid: Hsla) -> Canvas<()> {
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let height = f32::from(bounds.size.height);
            let width = f32::from(bounds.size.width);
            for quarter in [0.25, 0.5, 0.75] {
                let y = (bounds.top() + px(height * quarter)).floor();
                window.paint_quad(fill(
                    Bounds::new(point(bounds.left(), y), size(bounds.size.width, px(1.))),
                    grid,
                ));
            }
            // The newest sample at the right edge, a fixed step apart, so
            // that a curve with little history yet starts part way.
            let step = width / (CURVE_SAMPLES - 1) as f32;
            for (index, series) in curve.iter().enumerate() {
                if series.len() < 2 {
                    continue;
                }
                let count = series.len();
                let at = |i: usize, value: f32| {
                    point(
                        bounds.right() - px(step * (count - 1 - i) as f32),
                        bounds.top() + px(1. + (height - 2.) * (1. - value.clamp(0., 1.))),
                    )
                };
                if index == 0 {
                    let mut area = PathBuilder::fill();
                    area.move_to(point(at(0, 0.).x, bounds.bottom()));
                    for (i, value) in series.iter().enumerate() {
                        area.line_to(at(i, *value));
                    }
                    area.line_to(point(bounds.right(), bounds.bottom()));
                    area.close();
                    if let Ok(path) = area.build() {
                        window.paint_path(
                            path,
                            linear_gradient(
                                180.,
                                linear_color_stop(color.opacity(0.26), 0.),
                                linear_color_stop(color.opacity(0.04), 1.),
                            ),
                        );
                    }
                }
                let mut line = PathBuilder::stroke(px(if index == 0 { 2. } else { 1.5 }));
                for (i, value) in series.iter().enumerate() {
                    if i == 0 {
                        line.move_to(at(i, *value));
                    } else {
                        line.line_to(at(i, *value));
                    }
                }
                if let Ok(path) = line.build() {
                    window.paint_path(path, color.opacity(if index == 0 { 0.95 } else { 0.55 }));
                }
            }
        },
    )
}

/// A tile of the gauges: a ring for its first reading when that has a
/// scale, its name and value beside; its other readings in two columns
/// (four when it takes a whole row).
fn tile(gauges: &[Gauge], wide: bool, p: &Palette, cx: &App) -> Div {
    let first = &gauges[0];
    let top = match first.parts.as_slice() {
        [one, two] => div()
            .flex()
            .flex_col()
            .min_w_0()
            .child(name(first.name.clone(), 13., p))
            .child(
                div()
                    .flex()
                    .when(wide, |rates| rates.gap(px(24.)))
                    .when(!wide, |rates| rates.flex_col())
                    .children([one, two].map(|part| {
                        div()
                            .h(theme::font(24.))
                            .flex()
                            .items_center()
                            .child(rate(first, part, TALL, p, cx))
                    })),
            ),
        _ => div()
            .flex()
            .items_center()
            .gap(px(10.))
            .min_w_0()
            .when(first.bar, |top| {
                top.child(ring(first.fraction, tint(first, p), p.stroke_strong))
            })
            .child(
                div()
                    .flex()
                    .flex_col()
                    .min_w_0()
                    .child(name(first.name.clone(), 13., p))
                    .child(
                        div()
                            .h(theme::font(26.))
                            .flex()
                            .items_center()
                            .child(reading(first, BIG, p, cx)),
                    ),
            ),
    };
    let rest = gauges[1..].iter().collect::<Vec<_>>();
    div()
        .flex_grow(1.)
        // Two to a row where each gets 140 px, else one (a narrow preview).
        .flex_basis(px(140.))
        .min_w_0()
        .flex()
        .flex_col()
        .gap(px(8.))
        .p(px(10.))
        .bg(p.layer_strong)
        .border_1()
        .border_color(p.stroke)
        .rounded(px(6.))
        .child(top)
        .when(!rest.is_empty(), |tile| {
            tile.child(stats(&rest, if wide { 4 } else { 2 }, p, cx))
        })
}

/// A ring 46 px across filled clockwise from the top to `fraction`.
fn ring(fraction: Option<f64>, color: Hsla, track: Hsla) -> impl IntoElement {
    const STROKE: f32 = 6.;
    let fraction = fraction.unwrap_or(0.).clamp(0., 1.) as f32;
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let center = bounds.center();
            let radius = f32::from(bounds.size.width) / 2. - STROKE / 2.;
            let at = |angle: f32, radius: f32| {
                point(
                    center.x + px(radius * angle.cos()),
                    center.y + px(radius * angle.sin()),
                )
            };
            let tau = std::f32::consts::TAU;
            let arc = |from: f32, sweep: f32| {
                let mut path = PathBuilder::stroke(px(STROKE));
                let steps = ((sweep / tau) * 72.).ceil().max(2.) as usize;
                for step in 0..=steps {
                    let point = at(from + sweep * step as f32 / steps as f32, radius);
                    if step == 0 {
                        path.move_to(point);
                    } else {
                        path.line_to(point);
                    }
                }
                path.build().ok()
            };
            // Round ends, as the strokes end square.
            let dot = |angle: f32| {
                let middle = at(angle, radius);
                let mut path = PathBuilder::fill();
                for step in 0..=24 {
                    let around = tau * step as f32 / 24.;
                    let point = point(
                        middle.x + px(STROKE / 2. * around.cos()),
                        middle.y + px(STROKE / 2. * around.sin()),
                    );
                    if step == 0 {
                        path.move_to(point);
                    } else {
                        path.line_to(point);
                    }
                }
                path.close();
                path.build().ok()
            };
            if let Some(path) = arc(0., tau) {
                window.paint_path(path, track);
            }
            if fraction > 0. {
                let start = -tau / 4.;
                let sweep = tau * fraction;
                if let Some(path) = arc(start, sweep) {
                    window.paint_path(path, color);
                }
                for angle in [start, start + sweep] {
                    if let Some(path) = dot(angle) {
                        window.paint_path(path, color);
                    }
                }
            }
        },
    )
    .size(px(46.))
    .flex_none()
}

/// Where the strip's groups were laid out, by index, for the lines between
/// them and its size.
pub(in crate::frontend) type Placed = Rc<RefCell<Vec<Option<Bounds<Pixels>>>>>;

/// The strip: each group a meter for its first reading when that has a
/// scale, then a name over a value for each reading; a line between groups
/// in a row. It wraps between groups at `room` pixels; `extent` learns the
/// width and height it takes.
pub(in crate::frontend) fn strip(
    gauges: &[Gauge],
    room: f32,
    extent: Rc<Cell<Option<(f32, f32)>>>,
    p: &Palette,
    window: &Window,
    cx: &App,
) -> Div {
    let ranges = groups(gauges);
    let placed: Placed = Rc::new(RefCell::new(vec![None; ranges.len()]));
    let line = p.stroke_strong;
    let cell = |label: Div, value: Div, width: f32| {
        div()
            .flex()
            .flex_col()
            .child(label.h(theme::font(16.)).flex().items_center())
            .child(
                div()
                    .h(theme::font(22.))
                    .min_w(px(width))
                    .flex()
                    .items_center()
                    .child(value),
            )
    };
    // Made here, before the strip's own measure takes `placed`.
    let groups: Vec<Div> = ranges
        .into_iter()
        .enumerate()
        .map(|(index, range)| {
            let gauges = &gauges[range];
            let first = &gauges[0];
            let cells = gauges.iter().enumerate().flat_map(|(at, gauge)| {
                let title = if at == 0 {
                    gauge.name.clone()
                } else {
                    option(gauge)
                };
                match gauge.parts.as_slice() {
                    [_] => vec![cell(
                        name(title, 12., p),
                        reading(gauge, TALL, p, cx),
                        reserve(gauge, TALL, window, cx),
                    )],
                    parts => parts
                        .iter()
                        .enumerate()
                        .map(|(which, part)| {
                            let mark = div()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(metric_color(gauge.reading.metric, *p))
                                .child(part.mark.clone());
                            cell(
                                div()
                                    .flex()
                                    .gap(px(4.))
                                    .text_size(theme::font(12.))
                                    .text_color(p.muted)
                                    .when(which == 0, |label| label.child(title.clone()))
                                    .child(mark),
                                value(&part.text, TALL, p.ink, p.muted, cx),
                                value_width(&part.widest, TALL, window, cx)
                                    .max(value_width(&part.text, TALL, window, cx)),
                            )
                        })
                        .collect(),
                }
            });
            let placed = placed.clone();
            div()
                .relative()
                .flex()
                .items_center()
                .gap(px(14.))
                .px(px(16.))
                .when(first.bar, |group| {
                    group.child(
                        div()
                            .relative()
                            .flex_none()
                            .w(px(6.))
                            .h(px(36.))
                            .rounded(px(3.))
                            .bg(p.stroke_strong)
                            .overflow_hidden()
                            .child(
                                div()
                                    .absolute()
                                    .left_0()
                                    .right_0()
                                    .bottom_0()
                                    .h(relative(first.fraction.unwrap_or(0.) as f32))
                                    .rounded(px(3.))
                                    .bg(tint(first, p)),
                            ),
                    )
                })
                .children(cells)
                .child(
                    canvas(
                        move |bounds, _, _| {
                            if let Some(slot) = placed.borrow_mut().get_mut(index) {
                                *slot = Some(bounds);
                            }
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .inset_0(),
                )
        })
        .collect();
    div()
        .relative()
        .w(px(room))
        .flex()
        .flex_wrap()
        .items_stretch()
        .gap(px(8.))
        .py(px(8.))
        .px(px(2.))
        .children(groups)
        .child(
            canvas(
                move |bounds, _, _| {
                    // Reached after the groups: their places are known.
                    let placed = placed.borrow();
                    let right = placed
                        .iter()
                        .flatten()
                        .map(|group| f32::from(group.right()))
                        .fold(f32::from(bounds.left()), f32::max);
                    extent.set(Some((
                        right - f32::from(bounds.left()) + 2.,
                        f32::from(bounds.size.height),
                    )));
                    placed.clone()
                },
                move |_, placed, window, _| {
                    // A line before each group but the first of its row.
                    for pair in placed.windows(2) {
                        if let [Some(before), Some(group)] = pair
                            && (before.top() - group.top()).abs() < px(0.5)
                        {
                            window.paint_quad(fill(
                                Bounds::new(
                                    point(group.left(), group.top()),
                                    size(px(1.), group.size.height),
                                ),
                                line,
                            ));
                        }
                    }
                },
            )
            .absolute()
            .inset_0(),
        )
}

#[cfg(test)]
mod tests {
    use super::pieces;

    fn split(text: &str) -> Vec<(&str, bool)> {
        // Leaked for brevity: tests only.
        pieces(text)
            .into_iter()
            .map(|(piece, unit)| (&*Box::leak(piece.into_boxed_str()), unit))
            .collect()
    }

    #[test]
    fn values_split_into_figures_and_units() {
        assert_eq!(split("74%"), [("74", false), ("%", true)]);
        assert_eq!(split("3.94 GHz"), [("3.94", false), ("GHz", true)]);
        assert_eq!(split("3,94 GHz"), [("3,94", false), ("GHz", true)]);
        assert_eq!(
            split("5 天 14 小时"),
            [("5", false), ("天", true), ("14", false), ("小时", true)]
        );
        assert_eq!(split("42% TDP"), [("42", false), ("% TDP", true)]);
        // A space between digits groups thousands.
        assert_eq!(split("1 234 RPM"), [("1 234", false), ("RPM", true)]);
        assert_eq!(split("+45 W"), [("+45", false), ("W", true)]);
        assert_eq!(split("-15 W"), [("-15", false), ("W", true)]);
        assert_eq!(split("—"), [("—", false)]);
    }

    #[test]
    fn battery_power_groups_with_battery() {
        use super::groups;
        use crate::frontend::monitors::Gauge;
        use crate::settings::{Metric, Reading};

        let gauge = |metric: Metric| Gauge {
            reading: Reading::from(metric),
            name: String::new(),
            short: String::new(),
            parts: Vec::new(),
            fraction: None,
            bar: false,
            high: false,
            trend: None,
            pair: None,
        };
        let list = [
            gauge(Metric::Battery),
            gauge(Metric::BatteryPower),
            gauge(Metric::Uptime),
        ];
        assert_eq!(groups(&list), [0..2, 2..3]);
    }
}
