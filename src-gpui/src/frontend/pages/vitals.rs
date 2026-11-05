//! The vitals band over the process list: a card per resource with its figure,
//! readings and recent curve.
use super::*;
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
            hw["cpuMhz"].as_f64().map(|v| {
                (
                    format!("{:.2} GHz", v / 1000.),
                    label("performance.cpu.speed"),
                )
            }),
            performance::cpu_temperature(hw).map(|v| {
                (
                    format!("{v:.0} °C"),
                    label(if performance::has_die_temperature(hw) {
                        "common.resource.temperature"
                    } else {
                        "processes.vitals.acpiTemperature"
                    }),
                )
            }),
            performance::cpu_power(&hw["power"])
                .map(|(v, source)| (format!("{} W", charts::watts(v)), label(source))),
        ];
        let disk = values(&hw["disks"])
            .iter()
            .map(|d| number(d, "read") + number(d, "write"))
            .sum::<f64>();
        let busiest = values(&hw["disks"])
            .iter()
            .max_by(|a, b| number(a, "active").total_cmp(&number(b, "active")));
        // The busiest GPU; the first one listed while they are equally busy.
        let gpu = values(&hw["gpus"])
            .iter()
            .fold(None::<&Value>, |busiest, g| match busiest {
                Some(b) if number(g, "util") <= number(b, "util") => Some(b),
                _ => Some(g),
            });
        // Where the GPU figure comes from: Windows engines, process engines or NVML.
        let gpu_source = gpu.map(|g| {
            let source = g["utilSource"].as_str().unwrap_or("windows");
            label(&format!("performance.gpu.utilSource.{source}"))
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
        let gb = |bytes: f64| format!("{:.1}", bytes / 1073741824.);
        let split = |value: String| -> (String, String) {
            value
                .split_once(' ')
                .map(|(figure, unit)| (figure.to_owned(), unit.to_owned()))
                .unwrap_or((value, String::new()))
        };
        let disk_value = split(if disk < 1. {
            "0 KB/s".into()
        } else {
            theme::bytes(disk, true)
        });
        let net_value = split(if rx + tx < 1. {
            if bits {
                "0 Kbps".into()
            } else {
                "0 KB/s".into()
            }
        } else {
            theme::network(rx + tx, bits)
        });
        let mem_percent = (number(sys, "memUsed") / number(sys, "memTotal").max(1.) * 100.).round();
        let mut cards: Vec<VitalCard> = vec![
            (
                "cpu",
                "cpu".into(),
                "system.cpu",
                p.cpu,
                Some(100.),
                format!("{}", number(sys, "cpu").round()),
                "%".into(),
                hw["cpuMhz"]
                    .as_f64()
                    .map(|v| format!("{:.2} GHz", v / 1000.))
                    .unwrap_or_default(),
            ),
            (
                "memory",
                "memory".into(),
                "system.memUsed",
                p.memory,
                Some(number(sys, "memTotal").max(1.)),
                gb(number(sys, "memUsed")),
                format!("/ {} GB", gb(number(sys, "memTotal"))),
                format!("{mem_percent}%"),
            ),
            (
                "disk",
                format!(
                    "disk:{}",
                    busiest
                        .map(|d| d["info"]["index"].clone())
                        .unwrap_or(json!(0))
                ),
                "totals.disk",
                p.io,
                Some(charts::rate_ceiling(
                    peak("totals.disk"),
                    charts::RateScale::Disk,
                )),
                disk_value.0,
                disk_value.1,
                busiest
                    .map(|d| {
                        tr!(
                            "processes.vitals.diskActive",
                            value = number(d, "active").round() as u64
                        )
                    })
                    .unwrap_or_default(),
            ),
            (
                "network",
                format!(
                    "nic:{}",
                    values(&hw["nics"])
                        .first()
                        .map(|n| text(&n["info"], "id"))
                        .unwrap_or_default()
                ),
                "totals.network",
                p.network,
                Some(charts::rate_ceiling(
                    peak("totals.network"),
                    charts::RateScale::Network(bits),
                )),
                net_value.0,
                net_value.1,
                tr!(
                    "processes.vitals.netRates",
                    received = theme::network(rx, bits),
                    sent = theme::network(tx, bits)
                ),
            ),
        ];
        if let Some(gpu) = gpu {
            cards.push((
                "gpu",
                format!("gpu:{}", text(&gpu["info"], "id")),
                "totals.gpu",
                p.gpu,
                Some(100.),
                format!("{}", number(gpu, "util").round()),
                "%".into(),
                gpu["temperature"]
                    .as_f64()
                    .map(|v| format!("{v:.0} °C"))
                    .unwrap_or_default(),
            ));
        }
        let count = cards.len();
        let hover = p.hover;
        let press = p.press;
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
            .map(|card| if card.0 == "cpu" { 1.3 } else { 1. })
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
        let mut band = div()
            .flex()
            .flex_shrink_0()
            .h(theme::dimension("vitalsHeight"))
            .mx(px(12.))
            .border_1()
            .border_color(p.stroke)
            .rounded(px(8.))
            .bg(p.chart_surface);
        for (index, (id, device, key, color, max, figure, unit, detail)) in
            cards.into_iter().enumerate()
        {
            let (figure_size, unit_size, content) = fit(id, index);
            let plot = self.chart(
                &format!("vital-{id}"),
                &[(key, color)],
                max,
                theme::dimension("vitalsHeight") - px(63.),
                false,
                cx,
            );
            let name = if matches!(id, "cpu" | "gpu") {
                id.to_uppercase()
            } else {
                label(&format!("common.resource.{id}"))
            };
            let mut header = div()
                .relative()
                .flex()
                .w_full()
                .items_start()
                .justify_between()
                .gap(px(12.))
                .child(
                    div()
                        .flex_shrink_0()
                        .text_size(theme::font(12.))
                        .text_color(p.muted)
                        .child(name.clone()),
                );
            if id == "cpu" && !values(&sys["cores"]).is_empty() {
                header = header.child(self.core_comb(cx));
            } else if id == "gpu" {
                let gpu = gpu.expect("GPU card has a device");
                let metrics = [
                    gpu["temperature"]
                        .as_f64()
                        .map(|v| (format!("{v:.0} °C"), label("performance.gpu.temperature"))),
                    gpu["boardPower"]
                        .as_f64()
                        .map(|v| format!("{} W", charts::watts(v)))
                        .or_else(|| {
                            gpu["power"]
                                .as_f64()
                                .filter(|v| *v > 0.)
                                .map(|v| format!("{v:.0}% TDP"))
                        })
                        .map(|v| (v, label("performance.gpu.power"))),
                ];
                let available =
                    (content - theme::text_width(&name, theme::font(12.), window, cx) - px(12.))
                        .max(px(1.));
                let references = ["999 °C", "999% TDP"];
                let widths = references
                    .map(|v| self.vital_text_width(v, theme::font(12.), false, window, cx));
                let ratio = ((f32::from(available) - 6.).max(1.)
                    / f32::from(widths[0] + widths[1]).max(1.))
                .min(1.);
                let font = (theme::font(12.) * ratio).max(px(9.)).min(theme::font(12.));
                header = header.child(div().flex().gap(px(6.)).h(theme::font(12.) * 1.4).children(
                    metrics.into_iter().enumerate().map(|(i, value)| {
                        self.vital_metric(
                            if i == 0 {
                                "vital-gpu-temperature"
                            } else {
                                "vital-gpu-power"
                            },
                            value,
                            widths[i] * ratio,
                            font,
                        )
                    }),
                ));
            } else if id != "cpu" && !detail.is_empty() {
                header = header.child(
                    div()
                        .id(("vital-detail", index))
                        .min_w_0()
                        .truncate()
                        .text_size(theme::font(12.))
                        .text_color(p.muted)
                        .child(detail.clone())
                        .when(id == "network", |d| {
                            d.tooltip(|window, cx| {
                                widgets::tooltip(label("processes.vitals.netTitle"), window, cx)
                            })
                        }),
                );
            }
            let open = tr!("processes.vitals.open", device = name);
            let figure_tip = gpu_source.clone().filter(|_| id == "gpu");
            band = band.child(
                Button::new(format!("vital-{id}"))
                    .relative()
                    .flex_1()
                    .when(id == "cpu", |b| b.flex_grow(1.3))
                    .min_w_0()
                    .h_full()
                    .when(index == 0, |b| b.rounded_l(px(7.)))
                    .when(index + 1 == count, |b| b.rounded_r(px(7.)))
                    .pt(px(11.))
                    .px(px(16.))
                    // The card content sits in the
                    // middle of what the top padding leaves, in the body's 1.4
                    // line height.
                    .flex()
                    .flex_col()
                    .items_start()
                    .justify_center()
                    .line_height(relative(1.4))
                    .text_color(p.ink)
                    .hover(move |s| s.bg(hover))
                    .active(move |s| s.bg(press))
                    .when(index > 0, |d| d.border_l_1().border_color(p.stroke))
                    .accessibility_label(open.clone())
                    .tooltip(move |window, cx| widgets::tooltip(open.clone(), window, cx))
                    .child(
                        div()
                            .absolute()
                            .top(px(62.))
                            .bottom_0()
                            // Keep the ink on the opaque part of the rounded band at every DPI.
                            .left(px(if index == 0 { 8. } else { 0. }))
                            .right(px(if index + 1 == count { 8. } else { 0. }))
                            .child(plot),
                    )
                    .child(header)
                    .child(
                        div()
                            .id(("vital-figure", index))
                            .relative()
                            .mt(px(-2.))
                            .flex()
                            .items_baseline()
                            .gap(px(10.))
                            .w_full()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .child(
                                div()
                                    .flex()
                                    .items_baseline()
                                    .child(
                                        div()
                                            .font_family(theme::display_font(cx))
                                            .text_size(figure_size)
                                            .font_weight(FontWeight(330.))
                                            .line_height(relative(1.1))
                                            .child(figure),
                                    )
                                    .child(
                                        div()
                                            .ml(px(2.))
                                            .font_family(theme::display_font(cx))
                                            .text_size(unit_size)
                                            .text_color(p.muted)
                                            .child(unit),
                                    ),
                            )
                            .when(id == "cpu", |d| {
                                let left =
                                    self.vital_text_width("100", figure_size, true, window, cx)
                                        + self.vital_text_width("%", unit_size, true, window, cx)
                                        + px(12.);
                                let available = (content - left).max(px(1.));
                                let reference = ["99.99 GHz", "999 °C", "9999 W"];
                                let widths = reference.map(|v| {
                                    self.vital_text_width(v, theme::font(12.), false, window, cx)
                                });
                                let two_rows =
                                    available < widths.iter().copied().sum::<Pixels>() + px(12.);
                                let needed = if two_rows {
                                    widths[0].max(widths[1] + widths[2] + px(6.))
                                } else {
                                    widths.iter().copied().sum::<Pixels>() + px(12.)
                                };
                                let ratio =
                                    (f32::from(available) / f32::from(needed).max(1.)).min(1.);
                                let font =
                                    (theme::font(12.) * ratio).max(px(9.)).min(theme::font(12.));
                                let gap = px(6.) * ratio;
                                let line = font * 1.4;
                                let ids = [
                                    "vital-cpu-frequency",
                                    "vital-cpu-temperature",
                                    "vital-cpu-power",
                                ];
                                let group = div()
                                    .absolute()
                                    .left(left)
                                    .bottom(figure_size * 0.14)
                                    .w(available)
                                    .h(line * if two_rows { 2. } else { 1. })
                                    .flex()
                                    .when(two_rows, |d| d.flex_col())
                                    .when(!two_rows, |d| d.gap(gap))
                                    .child(self.vital_metric(
                                        ids[0],
                                        cpu_metrics[0].clone(),
                                        widths[0] * ratio,
                                        font,
                                    ))
                                    .child(div().flex().gap(gap).children((1..3).map(|i| {
                                        self.vital_metric(
                                            ids[i],
                                            cpu_metrics[i].clone(),
                                            widths[i] * ratio,
                                            font,
                                        )
                                    })));
                                d.child(group)
                            })
                            .when_some(figure_tip, |d, tip| {
                                d.tooltip(move |window, cx| {
                                    widgets::tooltip(tip.clone(), window, cx)
                                })
                            }),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.act(Action::Device(device.clone()), window, cx)
                    })),
            );
        }
        band.into_any_element()
    }
    /// Per-core load, one 4 px bar per logical processor over a faint track,
    /// gliding with the chart clock.
    fn core_comb(&self, cx: &mut Context<Self>) -> AnyElement {
        let keys = (0..values(&self.snapshot["system"]["cores"]).len())
            .map(|index| format!("system.cores.{index}"))
            .collect::<Vec<_>>();
        div()
            .id("vital-cores")
            .flex_shrink_0()
            .w(charts::bars_width(keys.len()))
            .h(px(26.))
            .child(self.chart_bars("vital-core-bars", &keys, self.palette.cpu, px(26.), cx))
            .tooltip(|window, cx| widgets::tooltip(label("processes.vitals.cores"), window, cx))
            .into_any_element()
    }
    fn vital_text_width(
        &self,
        text: &str,
        size: Pixels,
        display: bool,
        window: &Window,
        cx: &App,
    ) -> Pixels {
        let mut run = theme::text_run(
            text,
            if display {
                FontWeight(330.)
            } else {
                FontWeight::NORMAL
            },
            cx,
        );
        if display {
            run.font.family = theme::display_font(cx);
        }
        run.font.features = FontFeatures(Arc::new(vec![("tnum".into(), 1)]));
        window
            .text_system()
            .layout_line(text, size, &[run], None)
            .width
    }
    fn vital_metric(
        &self,
        id: &str,
        reading: Option<(String, String)>,
        width: Pixels,
        font: Pixels,
    ) -> Stateful<Div> {
        let anchors = self.ui.anchors.clone();
        let key: SharedString = id.to_owned().into();
        let hint = reading
            .as_ref()
            .map(|(value, hint)| format!("{hint}\n{value}"));
        div()
            .id(key.clone())
            .w(width)
            .h(font * 1.4)
            .flex_shrink_0()
            .min_w_0()
            .text_size(font)
            .text_color(self.palette.muted)
            .font_features(FontFeatures(Arc::new(vec![("tnum".into(), 1)])))
            .text_right()
            .truncate()
            .child(reading.map(|(value, _)| value).unwrap_or_default())
            .when_some(hint, |d, hint| {
                d.tooltip(move |window, cx| widgets::tooltip(hint.clone(), window, cx))
            })
            .when(self.options.test, |d| {
                d.on_prepaint(move |bounds, _, _| {
                    anchors.borrow_mut().insert(key.clone(), bounds);
                })
            })
    }
}

/// A vitals segment: id, device to open, series, colour, chart top, figure, unit, detail.
type VitalCard = (
    &'static str,
    String,
    &'static str,
    Hsla,
    Option<f64>,
    String,
    String,
    String,
);
