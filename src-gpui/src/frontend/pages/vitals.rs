//! The vitals band over the process list: a card per resource with its figure,
//! readings and recent curve.
//!
//! The curve fills its card under a headroom that holds the top of its scale,
//! and fades out to the left under the card's text. The chart layer draws
//! curves over the page, so it also draws that text and the scrim that fades
//! the curve out under it (`charts::Overlay`); the page lays the same text out
//! with transparent ink, for its tooltips and test anchors.
use super::*;
use charts::{Line, Look, Overlay, Scale};
use gpui_kit::gpui::transparent_black;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BandMode {
    Auto,
    Show,
    Hide,
}
impl BandMode {
    fn parse(value: &str) -> Self {
        match value {
            "show" => Self::Show,
            "hide" => Self::Hide,
            _ => Self::Auto,
        }
    }
    /// Whether the band shows over a process page `page` tall, where it
    /// needs `minimum` (the band, the header and ten rows). Before the page
    /// was first laid out (`None`), Automatic counts on the room it has in
    /// most windows.
    fn visible(self, page: Option<Pixels>, minimum: Pixels) -> bool {
        match self {
            Self::Auto => page.is_none_or(|page| page >= minimum),
            Self::Show => true,
            Self::Hide => false,
        }
    }
}

/// Which transition the band's height follows: a new one once the page's
/// height is known, so the first render that knows it shows the band as it
/// is then, without easing from the guess made before.
fn band_transition(measured: bool) -> (&'static str, &'static str) {
    (
        "process-vitals-height",
        if measured { "measured" } else { "unmeasured" },
    )
}

impl Desktop {
    fn vitals_page_minimum() -> Pixels {
        theme::dimension("vitalsHeight") + px(10.) + tables::process_table_height(10)
    }

    /// Measures the page area, which the process page fills, on every page
    /// and while the processes load: the process page's first render then
    /// already knows whether the band fits.
    pub(in crate::frontend) fn page_height_observer(&self, cx: &Context<Self>) -> AnyElement {
        let height = self.process_page_height.clone();
        let minimum = Self::vitals_page_minimum();
        let owner = cx.entity().downgrade();
        canvas(
            move |bounds, window, cx| {
                let fits = |page: Option<Pixels>| page.map(|page| page >= minimum);
                let previous = height.replace(Some(bounds.size.height));
                if fits(previous) != fits(Some(bounds.size.height)) {
                    let owner = owner.clone();
                    // Observe the full page, never the table enlarged by hiding the
                    // band. Only crossing the fixed budget needs a new page render.
                    window.defer(cx, move |_, cx| {
                        let _ = owner.update(cx, |this, cx| {
                            if this.page == "processes" {
                                cx.notify();
                            }
                        });
                    });
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .inset_0()
        .into_any_element()
    }

    fn show_vitals_band(&self) -> bool {
        BandMode::parse(&self.prefs.get::<String>("vitalsMode", "auto".into()))
            .visible(self.process_page_height.get(), Self::vitals_page_minimum())
    }

    pub(in crate::frontend) fn vitals_band(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // Keep the charts at their full size while revealing their viewport;
        // raster dimensions and history never shrink with the accordion.
        let full = theme::dimension("vitalsHeight") + px(10.);
        let target = if self.show_vitals_band() {
            full
        } else {
            px(0.)
        };
        let height = gpui_kit::base::motion::transition(
            band_transition(self.process_page_height.get().is_some()),
            target,
            theme::transition_slow(cx),
            window,
            cx,
        )
        .clamp(px(0.), full);
        let content = (height > px(0.)).then(|| {
            self.region(
                regions::RegionKind::Vitals,
                StyleRefinement::default()
                    .w_full()
                    .h(theme::dimension("vitalsHeight"))
                    .flex_shrink_0(),
                cx,
            )
        });
        let anchors = self.ui.anchors.clone();
        div()
            .relative()
            .w_full()
            .h(height)
            .flex_shrink_0()
            // At rest the band keeps its usual compositing path. Once closed,
            // omitting the region unregisters its charts and their frame ticks.
            .when(height < full, |d| d.overflow_hidden())
            .children(content)
            .when(self.options.test, |d| {
                d.child(
                    canvas(
                        move |bounds, _, _| {
                            anchors.borrow_mut().insert("vitals-band".into(), bounds);
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .inset_0(),
                )
            })
            .into_any_element()
    }

    pub(in crate::frontend) fn vitals_band_state(&self) -> Value {
        let shown = self.page == "processes" && self.show_vitals_band();
        let height = if self.page == "processes" {
            self.ui
                .anchors
                .borrow()
                .get("vitals-band")
                .map_or(0., |b| f32::from(b.size.height))
        } else {
            0.
        };
        let target = if shown {
            f32::from(theme::dimension("vitalsHeight")) + 10.
        } else {
            0.
        };
        json!({"targetVisible":shown,"items":self.rows.iter().filter(|row|row.section.is_none()).count(),"height":height,
            "pageHeight":self.process_page_height.get().map(f32::from),"requiredHeight":f32::from(Self::vitals_page_minimum()),
            "settled":(height-target).abs()<0.1,
            "activeCharts":self.active_charts.borrow().iter().filter(|key|key.starts_with("vital-")).count()})
    }
}

/// Across the card, where the scrim over the curve begins to fade and where
/// it has faded out.
const SCRIM: (f32, f32) = (0.22, 0.66);
/// The plot's bottom over the card's: the band's rounded bottom corners
/// leave the window's backdrop showing, where curves would stand out of the
/// band and Windows would draw them again with every frame. The curves meet
/// the cards' sides.
const BASELINE: f32 = 8.;
/// The band's corners inside its border.
const CORNER: f32 = 7.;
/// The scale's top line, in the series colour.
const SCALE_LINE: f32 = 0.3;
/// The curve's area fill at the top of the card, fading out downwards.
const FILL: f32 = 0.2;
/// The scale's label, in the headroom above its line.
const LABEL_TOP: f32 = 3.;

/// A vitals card before layout: id, device to open, series, colour, scale top
/// and its label, figure and unit, readings and the figure's tooltip.
struct Card {
    id: &'static str,
    device: String,
    key: &'static str,
    color: Hsla,
    top: f64,
    scale: String,
    figure: String,
    unit: String,
    readings: Readings,
    figure_tip: Option<String>,
}
enum Readings {
    /// One line, with its tooltip.
    Detail(String, Option<String>),
    /// Readings with their test anchors and tooltips (value, hint), in a font
    /// that fits the card.
    Metrics(Vec<(&'static str, (String, String))>, Pixels),
}
/// A vital card's text, laid out alike by the page and by the chart layer.
struct Face {
    index: usize,
    first: bool,
    last: bool,
    name: String,
    color: Hsla,
    figure: String,
    unit: String,
    figure_size: Pixels,
    unit_size: Pixels,
    figure_tip: Option<String>,
    readings: Readings,
    /// Where the scale's top is drawn, and its label in the headroom above.
    top: Pixels,
    scale: String,
    family: SharedString,
    display: SharedString,
    palette: Palette,
}
fn tabular() -> FontFeatures {
    FontFeatures(Arc::new(vec![("tnum".into(), 1)]))
}
impl Face {
    /// The text, filling the card: name, figure and readings. With `desktop`
    /// it takes transparent ink and its hit targets, as the page lays it out;
    /// without, it is drawn as it is, over the curve.
    fn body(&self, desktop: Option<&Desktop>) -> Div {
        let ink = |color: Hsla| {
            if desktop.is_some() {
                transparent_black()
            } else {
                color
            }
        };
        let tip = |text: &Option<String>| text.clone().filter(|_| desktop.is_some());
        let p = &self.palette;
        let small = theme::font(12.);
        let name = div()
            .flex()
            .items_center()
            .gap(px(6.))
            .h(small * 1.4)
            .text_size(small)
            .text_color(ink(p.muted))
            .whitespace_nowrap()
            .child(
                div()
                    .flex_shrink_0()
                    .size(px(6.))
                    .rounded_full()
                    .bg(ink(self.color)),
            )
            .child(self.name.clone());
        let figure = div()
            .id(("vital-figure", self.index))
            .flex()
            .items_baseline()
            .min_w_0()
            .overflow_hidden()
            .whitespace_nowrap()
            .font_family(self.display.clone())
            .font_features(tabular())
            .child(
                div()
                    .text_size(self.figure_size)
                    .font_weight(FontWeight(330.))
                    .line_height(relative(1.05))
                    .text_color(ink(p.ink))
                    .child(self.figure.clone()),
            )
            .child(
                div()
                    .ml(px(3.))
                    .text_size(self.unit_size)
                    .text_color(ink(p.muted))
                    .child(self.unit.clone()),
            )
            .when_some(tip(&self.figure_tip), |d, tip| {
                d.tooltip(move |window, cx| widgets::tooltip(tip.clone(), window, cx))
            });
        let readings = match &self.readings {
            Readings::Detail(text, hint) => div()
                .id(("vital-detail", self.index))
                .min_w_0()
                .truncate()
                .text_size(small)
                .font_features(tabular())
                .text_color(ink(p.muted))
                .child(text.clone())
                .when_some(tip(hint), |d, hint| {
                    d.tooltip(move |window, cx| widgets::tooltip(hint.clone(), window, cx))
                })
                .into_any_element(),
            Readings::Metrics(metrics, font) => div()
                .flex()
                .gap(*font * 0.75)
                .h(*font * 1.4)
                .min_w_0()
                .overflow_hidden()
                .children(metrics.iter().map(|(id, reading)| {
                    match desktop {
                        Some(desktop) => desktop
                            .vital_metric(id, reading.clone(), *font)
                            .into_any_element(),
                        None => div()
                            .flex_shrink_0()
                            .h(*font * 1.4)
                            .text_size(*font)
                            .font_features(tabular())
                            .text_color(p.muted)
                            .child(reading.0.clone())
                            .into_any_element(),
                    }
                }))
                .into_any_element(),
        };
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .pt(px(11.))
            .px(px(16.))
            .flex()
            .flex_col()
            .gap(px(5.))
            .overflow_hidden()
            .font_family(self.family.clone())
            .line_height(relative(1.4))
            .child(name)
            .child(figure)
            .child(readings)
    }
    /// The scale's top line across the card and its label in the headroom
    /// above. The page draws them, under the curve, which keeps below them:
    /// what the chart layer draws over the curve Windows has to blend into
    /// every frame of the moving curve it covers.
    fn scale(&self) -> Div {
        div()
            .absolute()
            .inset_0()
            .child(
                div()
                    .absolute()
                    .top(self.top - px(0.5))
                    .left_0()
                    .right_0()
                    .h(px(1.))
                    .bg(self.color.opacity(SCALE_LINE)),
            )
            .child(
                div()
                    .absolute()
                    .top(px(LABEL_TOP))
                    .right(px(16.))
                    .font_family(self.family.clone())
                    .text_size(theme::font(11.))
                    .line_height(relative(1.4))
                    .font_features(tabular())
                    .text_color(self.palette.muted)
                    .whitespace_nowrap()
                    .child(self.scale.clone()),
            )
    }
    /// What the chart layer draws over the curve: the scrim that fades the
    /// curve out under the text (only as far as it is not yet clear), the
    /// card's hover and press tints, and the text.
    fn overlay(&self) -> AnyElement {
        let p = self.palette;
        let group = SharedString::from(format!("vital-face-{}", self.index));
        let (first, last) = (self.first, self.last);
        let round = move |on: bool| if on { px(CORNER) } else { px(0.) };
        let scrim = canvas(
            |_, _, _| {},
            move |card, _, window, _| {
                let piece = Bounds::new(
                    card.origin,
                    size(card.size.width * SCRIM.1, card.size.height),
                );
                let corners = Corners {
                    top_left: round(first),
                    bottom_left: round(first),
                    ..Corners::default()
                };
                charts::paint_fade(window, card, piece, corners, p.chart_surface, SCRIM);
            },
        )
        .absolute()
        .inset_0();
        div()
            .id(group.clone())
            .group(group.clone())
            .relative()
            .size_full()
            .child(scrim)
            // Over the curve and the scrim alike, so that the whole card takes
            // one tint (the page leaves its own out).
            .child(
                div()
                    .id("vital-tint")
                    .absolute()
                    .inset_0()
                    .when(first, |d| d.rounded_l(px(CORNER)))
                    .when(last, |d| d.rounded_r(px(CORNER)))
                    .group_hover(group.clone(), move |s| s.bg(p.hover))
                    .group_active(group, move |s| s.bg(p.press)),
            )
            .child(self.body(None))
            .into_any_element()
    }
}
impl Desktop {
    pub(in crate::frontend) fn vitals(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.palette;
        let sys = &self.snapshot["system"];
        let hw = &self.snapshot["hardware"];
        let cpu_metrics = [
            (
                "vital-cpu-frequency",
                hw["cpuMhz"].as_f64().map(|v| {
                    (
                        crate::regional::number!("{:.2} GHz", v / 1000.),
                        label("performance.cpu.speed"),
                    )
                }),
            ),
            (
                "vital-cpu-temperature",
                performance::cpu_temperature(hw).map(|v| {
                    (
                        crate::regional::number!("{v:.0} °C"),
                        label(if performance::has_die_temperature(hw) {
                            "common.resource.temperature"
                        } else {
                            "processes.vitals.acpiTemperature"
                        }),
                    )
                }),
            ),
            (
                "vital-cpu-power",
                performance::cpu_power(&hw["power"])
                    .map(|(v, source)| (format!("{} W", charts::watts(v)), label(source))),
            ),
        ];
        let disk = model::disk_rate(values(&hw["disks"]));
        let busiest = values(&hw["disks"])
            .iter()
            .filter(|disk| disk["active"].is_number())
            .max_by(|a, b| number(a, "active").total_cmp(&number(b, "active")));
        // The busiest GPU; the first one listed while they are equally busy.
        let gpu = values(&hw["gpus"])
            .iter()
            .fold(None::<&Value>, |busiest, g| match busiest {
                Some(b) if number(g, "util") <= number(b, "util") => Some(b),
                _ => Some(g),
            });
        let rx = values(&hw["nics"])
            .iter()
            .map(|n| number(n, "rx"))
            .sum::<f64>();
        let tx = values(&hw["nics"])
            .iter()
            .map(|n| number(n, "tx"))
            .sum::<f64>();
        let bits = self.prefs.get::<String>("netUnit", "bytes".into()) == "bits";
        // Overview charts share throughput steps with device and history charts.
        let peak = |key: &str| {
            self.timeline
                .samples
                .get(key)
                .map(|s| s.iter().filter_map(|(_, v)| *v).fold(0., f64::max))
                .unwrap_or(0.)
        };
        let gb = |bytes: f64| crate::regional::number!("{:.1}", bytes / 1073741824.);
        let split = |value: String| -> (String, String) {
            value
                .split_once(' ')
                .map(|(figure, unit)| (figure.to_owned(), unit.to_owned()))
                .unwrap_or((value, String::new()))
        };
        let disk_value = split(
            disk.map(|value| {
                if value < 1. {
                    "0 KB/s".into()
                } else {
                    theme::bytes(value, true)
                }
            })
            .unwrap_or_else(|| label("format.missing")),
        );
        let net_value = split(if rx + tx < 1. {
            if bits {
                "0 Kbps".into()
            } else {
                "0 KB/s".into()
            }
        } else {
            theme::network(rx + tx, bits)
        });
        let mem_total = number(sys, "memTotal").max(1.);
        let mem_percent = (number(sys, "memUsed") / mem_total * 100.).round();
        let disk_top = charts::rate_ceiling(peak("totals.disk"), charts::RateScale::Disk);
        let net_top =
            charts::rate_ceiling(peak("totals.network"), charts::RateScale::Network(bits));
        let known = |metrics: Vec<(&'static str, Option<(String, String)>)>| {
            Readings::Metrics(
                metrics
                    .into_iter()
                    .filter_map(|(id, reading)| Some((id, reading?)))
                    .collect(),
                theme::font(12.),
            )
        };
        let mut cards = vec![
            Card {
                id: "cpu",
                device: "cpu".into(),
                key: "system.cpu",
                color: p.cpu,
                top: 100.,
                scale: "100%".into(),
                figure: format!("{}", number(sys, "cpu").round()),
                unit: "%".into(),
                readings: known(cpu_metrics.into()),
                figure_tip: None,
            },
            Card {
                id: "memory",
                device: "memory".into(),
                key: "system.memUsed",
                color: p.memory,
                top: mem_total,
                scale: format!("{} GB", gb(mem_total)),
                figure: gb(number(sys, "memUsed")),
                unit: format!("/ {} GB", gb(mem_total)),
                readings: Readings::Detail(format!("{mem_percent}%"), None),
                figure_tip: None,
            },
            Card {
                id: "disk",
                device: format!(
                    "disk:{}",
                    busiest
                        .or_else(|| values(&hw["disks"]).first())
                        .map(|d| d["info"]["index"].clone())
                        .unwrap_or(json!(0))
                ),
                key: "totals.disk",
                color: p.io,
                top: disk_top,
                scale: charts::rate_ceiling_label(disk_top, false),
                figure: disk_value.0,
                unit: disk_value.1,
                readings: Readings::Detail(
                    busiest
                        .map(|d| {
                            tr!(
                                "processes.vitals.diskActive",
                                value = number(d, "active").round() as u64
                            )
                        })
                        .unwrap_or_default(),
                    None,
                ),
                figure_tip: None,
            },
            Card {
                id: "network",
                device: format!(
                    "nic:{}",
                    values(&hw["nics"])
                        .first()
                        .map(|n| text(&n["info"], "id"))
                        .unwrap_or_default()
                ),
                key: "totals.network",
                color: p.network,
                top: net_top,
                scale: charts::rate_ceiling_label(net_top, bits),
                figure: net_value.0,
                unit: net_value.1,
                readings: Readings::Detail(
                    tr!(
                        "processes.vitals.netRates",
                        received = theme::network(rx, bits),
                        sent = theme::network(tx, bits)
                    ),
                    Some(label("processes.vitals.netTitle")),
                ),
                figure_tip: None,
            },
        ];
        if let Some(gpu) = gpu {
            let power = gpu["boardPower"]
                .as_f64()
                .map(|v| format!("{} W", charts::watts(v)))
                .or_else(|| {
                    gpu["power"]
                        .as_f64()
                        .filter(|v| *v > 0.)
                        .map(|v| crate::regional::number!("{v:.0}% TDP"))
                })
                .map(|v| (v, label("performance.gpu.power")));
            // Where the GPU figure comes from: Windows engines, process engines or NVML.
            let source = gpu["utilSource"].as_str().unwrap_or("windows");
            cards.push(Card {
                id: "gpu",
                device: format!("gpu:{}", text(&gpu["info"], "id")),
                key: "totals.gpu",
                color: p.gpu,
                top: 100.,
                scale: "100%".into(),
                figure: format!("{}", number(gpu, "util").round()),
                unit: "%".into(),
                readings: known(vec![
                    (
                        "vital-gpu-temperature",
                        gpu["temperature"].as_f64().map(|v| {
                            (
                                crate::regional::number!("{v:.0} °C"),
                                label("performance.gpu.temperature"),
                            )
                        }),
                    ),
                    ("vital-gpu-power", power),
                ]),
                figure_tip: Some(label(&format!("performance.gpu.utilSource.{source}"))),
            });
        }
        let count = cards.len();
        // The figures fit their card, which large text or scale can make
        // narrow: `clamp(16px, 33cqw, 34rem / 13)` of the card's content width,
        // the unit `clamp(9px, 15cqw, 15rem / 13)`. The band spans the page
        // inside its margins and border; the CPU card is 1.3 parts.
        let navigation = if self.nav_top {
            px(0.)
        } else {
            px(self.side_nav_width())
        };
        let band_width = window.viewport_size().width - navigation - px(2. * 12. + 2.);
        let parts = cards
            .iter()
            .map(|card| if card.id == "cpu" { 1.3 } else { 1. })
            .sum::<f32>();
        let fit = |id: &str, index: usize| {
            let share = if id == "cpu" { 1.3 } else { 1. } / parts.max(1.);
            let border = if index > 0 { px(1.) } else { px(0.) };
            let content = (band_width * share - px(2. * 16.) - border).max(px(0.));
            (
                (content * 0.33)
                    .min(theme::font_dimension("figureSize"))
                    .max(px(16.)),
                (content * 0.15).min(theme::font(15.)).max(px(9.)),
                content,
            )
        };
        // The scale's top sits under a headroom that holds its label: the
        // chart's top is raised so that the scale's top is drawn there.
        let height = theme::dimension("vitalsHeight") - px(2. + BASELINE);
        let headroom = px(LABEL_TOP) + theme::font(11.) * 1.4 + px(2.);
        let stroke = theme::dimension("chartStrokeWidth");
        let stretch = f64::from(f32::from(height - stroke / 2.))
            / f64::from(f32::from(height - headroom).max(1.));
        let family = cx.theme().font_family.clone();
        let display = theme::display_font(cx);
        let mut band = div()
            .flex()
            .flex_shrink_0()
            .h(theme::dimension("vitalsHeight"))
            .mx(px(12.))
            .border_1()
            .border_color(p.stroke)
            .rounded(px(8.))
            .bg(p.chart_surface);
        for (index, card) in cards.into_iter().enumerate() {
            let Card {
                id,
                device,
                key,
                color,
                top,
                scale,
                figure,
                unit,
                readings,
                figure_tip,
            } = card;
            // A card opens a device the performance page's list shows.
            let device = self.device_target(id, device);
            let (figure_size, unit_size, content) = fit(id, index);
            let readings = match readings {
                Readings::Metrics(metrics, small) => {
                    // Smaller text before cut readings, as the card narrows.
                    let needed = metrics
                        .iter()
                        .map(|(_, (value, _))| self.vital_text_width(value, small, window, cx))
                        .sum::<Pixels>()
                        + small * 0.75 * metrics.len().saturating_sub(1) as f32;
                    let ratio = (f32::from(content) / f32::from(needed).max(1.)).min(1.);
                    Readings::Metrics(metrics, (small * ratio).max(px(9.)).min(small))
                }
                detail => detail,
            };
            let name = if matches!(id, "cpu" | "gpu") {
                id.to_uppercase()
            } else {
                label(&format!("common.resource.{id}"))
            };
            let face = Rc::new(Face {
                index,
                first: index == 0,
                last: index + 1 == count,
                name: name.clone(),
                color,
                figure,
                unit,
                figure_size,
                unit_size,
                figure_tip,
                readings,
                top: headroom,
                scale,
                family: family.clone(),
                display: display.clone(),
                palette: p,
            });
            let overlay = Rc::new(Overlay {
                inset: Edges {
                    bottom: px(BASELINE),
                    ..Edges::default()
                },
                covers: SCRIM.0,
                build: Box::new({
                    let face = face.clone();
                    move || face.overlay()
                }),
            });
            let plot = self.chart_view(
                &format!("vital-{id}"),
                &[Line::new(key, color).fill(FILL)],
                Some(Scale::Fixed(top * stretch)),
                height,
                Look {
                    overlay: Some(overlay),
                    ..Look::default()
                },
                cx,
            );
            let open = tr!("processes.vitals.open", device = name);
            band = band.child(
                Button::new(format!("vital-{id}"))
                    .relative()
                    .flex_1()
                    .when(id == "cpu", |b| b.flex_grow(1.3))
                    .min_w_0()
                    .h_full()
                    .when(index == 0, |b| b.rounded_l(px(CORNER)))
                    .when(index + 1 == count, |b| b.rounded_r(px(CORNER)))
                    .when(index > 0, |d| d.border_l_1().border_color(p.stroke))
                    .accessibility_label(open.clone())
                    .tooltip(move |window, cx| widgets::tooltip(open.clone(), window, cx))
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .left_0()
                            .right_0()
                            .bottom(px(BASELINE))
                            .child(plot),
                    )
                    .child(face.scale())
                    .child(face.body(Some(self)))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.act(Action::Device(device.clone()), window, cx)
                    })),
            );
        }
        band.into_any_element()
    }
    /// The width of a reading at `size`, in tabular figures.
    fn vital_text_width(&self, text: &str, size: Pixels, window: &Window, cx: &App) -> Pixels {
        let mut run = theme::text_run(text, FontWeight::NORMAL, cx);
        run.font.features = tabular();
        window
            .text_system()
            .layout_line(text, size, &[run], None)
            .width
    }
    /// A reading as the page lays it out: transparent (the chart layer draws
    /// it), with its tooltip and test anchor.
    fn vital_metric(
        &self,
        id: &str,
        (value, hint): (String, String),
        font: Pixels,
    ) -> Stateful<Div> {
        let anchors = self.ui.anchors.clone();
        let key: SharedString = id.to_owned().into();
        let hint = format!("{hint}\n{value}");
        div()
            .id(key.clone())
            .flex_shrink_0()
            .h(font * 1.4)
            .text_size(font)
            .text_color(transparent_black())
            .font_features(tabular())
            .child(value)
            .tooltip(move |window, cx| widgets::tooltip(hint.clone(), window, cx))
            // Measured over the reading's own box: `on_prepaint`'s probe sits
            // where the flow leaves it, under the text.
            .when(self.options.test, |d| {
                d.child(
                    canvas(
                        move |bounds, _, _| {
                            anchors.borrow_mut().insert(key.clone(), bounds);
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .inset_0(),
                )
            })
    }
}

#[cfg(test)]
mod band_tests {
    use super::{BandMode, band_transition};
    use gpui_kit::px;

    #[test]
    fn automatic_band_depends_only_on_room_and_overrides_are_explicit() {
        let minimum = px(480.);
        for (page, room) in [(px(479.), false), (px(480.), true), (px(900.), true)] {
            assert_eq!(BandMode::Auto.visible(Some(page), minimum), room);
            assert!(BandMode::Show.visible(Some(page), minimum));
            assert!(!BandMode::Hide.visible(Some(page), minimum));
        }
        assert_eq!(BandMode::parse("show"), BandMode::Show);
        assert_eq!(BandMode::parse("hide"), BandMode::Hide);
        for saved in ["", "auto", "invalid"] {
            assert_eq!(BandMode::parse(saved), BandMode::Auto);
        }
    }

    #[test]
    fn the_first_render_shows_the_band_settled() {
        let minimum = px(480.);
        // Before the page is measured, Automatic shows the band rather than
        // growing it from nothing once the height arrives.
        assert!(BandMode::Auto.visible(None, minimum));
        assert!(BandMode::Show.visible(None, minimum));
        assert!(!BandMode::Hide.visible(None, minimum));
        // The first measured render starts a transition of its own, which
        // adopts its first target at once; later changes ease in it.
        assert_ne!(band_transition(false), band_transition(true));
        assert_eq!(band_transition(true), band_transition(true));
    }
}
