//! The CPU details page: processor information, sensor charts and driver readings as
//! cards that stay open, in the original order.
use super::*;

/// Per-core table columns: timeline field, label, scale, digits and unit.
struct Column {
    field: &'static str,
    label: &'static str,
    scale: f64,
    digits: usize,
    unit: &'static str,
    /// Short name in the idle summary.
    short: &'static str,
}
const fn column(
    field: &'static str,
    label: &'static str,
    scale: f64,
    digits: usize,
    unit: &'static str,
    short: &'static str,
) -> Column {
    Column {
        field,
        label,
        scale,
        digits,
        unit,
        short,
    }
}
const CORE_COLUMNS: [Column; 9] = [
    column(
        "clockMhz",
        "performance.cpu.columnClock",
        0.001,
        2,
        "GHz",
        "",
    ),
    column(
        "effectiveMhz",
        "performance.cpu.columnEffective",
        0.001,
        2,
        "GHz",
        "",
    ),
    column(
        "celsius",
        "performance.cpu.columnTemperature",
        1.,
        0,
        "°C",
        "",
    ),
    column(
        "distanceToLimit",
        "performance.cpu.columnHeadroom",
        1.,
        0,
        "°C",
        "",
    ),
    column("watts", "performance.cpu.columnPower", 1., 2, "W", ""),
    column("vid", "performance.cpu.columnVid", 1., 3, "V", ""),
    column("voltage", "performance.cpu.columnVoltage", 1., 3, "V", ""),
    column(
        "multiplier",
        "performance.cpu.columnMultiplier",
        1.,
        2,
        "×",
        "",
    ),
    column(
        "c0Percent",
        "performance.cpu.columnActivity",
        1.,
        1,
        "%",
        "",
    ),
];
const IDLE_COLUMNS: [Column; 6] = [
    column(
        "smuC0Percent",
        "performance.cpu.columnSmuC0",
        1.,
        1,
        "%",
        "C0",
    ),
    column("c1Percent", "performance.cpu.columnC1", 1., 1, "%", "C1"),
    column("c6Percent", "performance.cpu.columnC6", 1., 1, "%", "CC6"),
    column(
        "osC1Percent",
        "performance.cpu.columnOsC1",
        1.,
        1,
        "%",
        "C1",
    ),
    column(
        "osC2Percent",
        "performance.cpu.columnOsC2",
        1.,
        1,
        "%",
        "C2",
    ),
    column(
        "osC3Percent",
        "performance.cpu.columnOsC3",
        1.,
        1,
        "%",
        "C3",
    ),
];
const GROUPS: [(&str, &str); 4] = [
    ("power", "performance.cpu.telemetryPower"),
    ("voltage", "performance.cpu.telemetryVoltage"),
    ("clock", "performance.cpu.telemetryClock"),
    ("temperature", "performance.cpu.telemetryTemperature"),
];
const OS_IDLE: [&str; 3] = ["c1", "c2", "c3"];

fn reading(value: Option<f64>, digits: usize) -> String {
    value
        .filter(|v| v.is_finite())
        .map(|v| format!("{v:.digits$}"))
        .unwrap_or_else(missing)
}
fn columns_of<'a>(list: &'a [Column], cores: &[Value]) -> Vec<&'a Column> {
    list.iter()
        .filter(|column| cores.iter().any(|core| core[column.field].is_number()))
        .collect()
}
fn cell(core: &Value, column: &Column) -> String {
    reading(
        core[column.field].as_f64().map(|v| v * column.scale),
        column.digits,
    )
}
fn join(parts: Vec<Option<String>>) -> String {
    parts.into_iter().flatten().collect::<Vec<_>>().join(" · ")
}
/// "54–61 °C": the lowest and highest readings, or one value when they agree.
fn extent(values: impl Iterator<Item = f64>, digits: usize, unit: &str) -> Option<String> {
    let list = values.filter(|v| v.is_finite()).collect::<Vec<_>>();
    if list.is_empty() {
        return None;
    }
    let low = reading(list.iter().copied().reduce(f64::min), digits);
    let high = reading(list.iter().copied().reduce(f64::max), digits);
    Some(if low == high {
        format!("{low} {unit}")
    } else {
        format!("{low}–{high} {unit}")
    })
}
/// Mean of a Windows idle class across logical processors, or none if any lacks it.
fn os_idle(idle: &[Value], key: &str) -> Option<f64> {
    if idle.is_empty() || idle.iter().any(|value| !value[key].is_number()) {
        return None;
    }
    Some(idle.iter().map(|value| number(value, key)).sum::<f64>() / idle.len() as f64)
}
fn limit_text(flags: &[Value], history: bool) -> String {
    if flags.is_empty() {
        return missing();
    }
    let reasons = flags
        .iter()
        .filter(|flag| boolean(flag, if history { "occurred" } else { "active" }))
        .map(|flag| label(&format!("performance.cpu.limits.{}", text(flag, "reason"))))
        .collect::<Vec<_>>();
    if reasons.is_empty() {
        label(if history {
            "performance.cpu.limitNever"
        } else {
            "performance.cpu.throttleInactive"
        })
    } else {
        reasons.join(&label("common.listSeparator"))
    }
}
fn metric_metadata() -> &'static Value {
    static METRICS: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    METRICS.get_or_init(|| {
        serde_json::from_str(include_str!("../../../../../resources/cpu-metrics.json"))
            .expect("CPU metric metadata")
    })
}

impl Desktop {
    pub(in crate::frontend) fn cpu_details(
        &self,
        hw: &Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.palette;
        let cpu = &hw["cpu"];
        let sensors = &hw["cpuSensors"];
        let mut cards = Vec::new();

        // Processor information.
        let plain = |value: String| div().child(value).into_any_element();
        let mut rows = Vec::new();
        if !text(cpu, "vendor").is_empty() {
            rows.push((
                label("performance.cpu.vendor"),
                plain(text(cpu, "vendor").into()),
            ));
            rows.push((
                label("performance.cpu.identity"),
                plain(format!(
                    "0x{:X} / 0x{:X} / {}",
                    cpu["family"].as_u64().unwrap_or(0),
                    cpu["model"].as_u64().unwrap_or(0),
                    cpu["stepping"]
                )),
            ));
        }
        if let Some(microcode) = cpu["microcode"].as_u64() {
            rows.push((
                label("performance.cpu.microcode"),
                plain(format!("0x{microcode:X}")),
            ));
        }
        let zones = values(&hw["thermal"])
            .iter()
            .map(|zone| text(zone, "name"))
            .collect::<Vec<_>>();
        rows.push((
            label("performance.cpu.thermalSource"),
            plain(if has_die_temperature(hw) {
                label("performance.cpu.driverSource")
            } else if !zones.is_empty() {
                tr!(
                    "performance.cpu.thermalZones",
                    zones = zones.join(&label("common.listSeparator"))
                )
            } else {
                label("performance.cpu.thermalNone")
            }),
        ));
        if sensors.is_object() {
            rows.push((
                label("performance.cpu.driverStatus"),
                plain(label(&format!(
                    "performance.cpu.sensorState.{}",
                    match text(sensors, "state") {
                        "" => "pending",
                        state => state,
                    }
                ))),
            ));
        }
        let mut info = self.info_list(rows);
        let instructions = values(&cpu["instructions"])
            .iter()
            .filter_map(Value::as_str)
            .map(String::from)
            .collect::<Vec<_>>();
        if !instructions.is_empty() {
            // Instruction sets as tags under their label, across both columns.
            info = div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .child(info)
                .child(
                    div()
                        .text_color(p.muted)
                        .child(label("performance.cpu.instructions")),
                )
                .child(div().flex().flex_wrap().gap(px(4.)).mt(px(-4.)).children(
                    instructions.into_iter().map(|name| {
                        div()
                            .px(px(6.))
                            .py(px(1.))
                            .rounded(px(4.))
                            .bg(p.ink.opacity(0.06))
                            .text_color(p.muted)
                            .text_size(theme::font(11.))
                            .line_height(px(16.))
                            .child(name)
                    }),
                ))
                .into_any_element();
        }
        cards.push(self.card(
            label("performance.cpu.sectionInfo"),
            None,
            None,
            false,
            info,
        ));

        // Temperature and power charts.
        let temperature = temperature_key(hw);
        let package = hw["power"]["package"].as_f64();
        if temperature.is_some() || package.is_some() {
            let span = charts::span_label(self.interval);
            let series = |key: &str| {
                self.timeline
                    .samples
                    .get(&model::series_key(&self.snapshot, key))
                    .map(|list| {
                        list.iter()
                            .rev()
                            .take(60)
                            .filter_map(|(_, v)| *v)
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default()
            };
            let summary = join(vec![
                temperature
                    .as_deref()
                    .and_then(|key| extent(series(key).into_iter(), 0, "°C")),
                package.and_then(|_| {
                    let list = series("hardware.power.package");
                    if list.is_empty() {
                        return None;
                    }
                    let low = charts::watts(list.iter().copied().fold(f64::INFINITY, f64::min));
                    let high = charts::watts(list.iter().copied().fold(0., f64::max));
                    Some(if low == high {
                        format!("{low} W")
                    } else {
                        format!("{low}–{high} W")
                    })
                }),
            ]);
            let title = label(match (temperature.is_some(), package.is_some()) {
                (false, _) => "performance.cpu.charts.power",
                (true, false) => "performance.cpu.charts.temperature",
                (true, true) => "performance.cpu.charts.both",
            });
            // `clamp(110px, 22cqh, 220px)` of the detail card (until it is
            // laid out, of the viewport).
            let card = self
                .perf_card_height(window)
                .unwrap_or_else(|| window.viewport_size().height);
            let chart_height = px((f32::from(card) * 0.22).clamp(110., 220.));
            let mut pair = div().grid().grid_cols(2).gap(px(16.));
            if let Some(key) = &temperature {
                pair = pair.child(
                    self.chart_view(
                        "cpu-temperature",
                        &[Line::new(key.clone(), p.cpu)],
                        Some(Scale::Fixed(100.)),
                        chart_height,
                        Look::boxed(label("common.resource.temperature"))
                            .top("100 °C")
                            .format(MetricFormat::Celsius),
                        cx,
                    ),
                );
            }
            if package.is_some() {
                pair = pair.child(
                    self.chart_view(
                        "cpu-power",
                        &[
                            Line::new("hardware.power.package", p.cpu)
                                .label(label("performance.cpu.powerSeries.package")),
                            Line::new("hardware.power.cores", p.cpu)
                                .dashed()
                                .label(label("performance.cpu.powerSeries.cores")),
                        ],
                        Some(Scale::Round {
                            floor: 5.,
                            unit: "W",
                        }),
                        chart_height,
                        Look::boxed(label("performance.cpu.power")).format(MetricFormat::Watts),
                        cx,
                    ),
                );
            }
            cards.push(
                self.card(
                    title,
                    None,
                    (!summary.is_empty())
                        .then(|| tr!("performance.cpu.chartsRange", span = span, values = summary)),
                    false,
                    pair,
                ),
            );
        }

        if sensors.is_object() {
            cards.push(self.card(
                label("performance.cpu.telemetryTitle"),
                None,
                Some(self.sensor_summary(sensors, package)).filter(|s| !s.is_empty()),
                false,
                self.sensor_groups(sensors, self.detail_width(window), cx),
            ));
            let cores = values(&sensors["cores"]);
            if !cores.is_empty() && !columns_of(&CORE_COLUMNS, cores).is_empty() {
                let summary = join(vec![
                    Some(tr!("performance.cpu.coreCount", count = cores.len())),
                    extent(
                        cores
                            .iter()
                            .filter_map(|c| c["clockMhz"].as_f64())
                            .map(|v| v / 1000.),
                        2,
                        "GHz",
                    ),
                    extent(cores.iter().filter_map(|c| c["celsius"].as_f64()), 0, "°C"),
                ]);
                cards.push(self.card(
                    label("performance.cpu.perCore"),
                    None,
                    Some(summary),
                    false,
                    self.core_table(cores, cx),
                ));
            }
            let board = !values(&sensors["boardFans"]).is_empty()
                || !values(&sensors["boardMetrics"]).is_empty();
            let limits = values(&sensors["limits"]);
            if board || !limits.is_empty() {
                let mut pair = Vec::new();
                if board {
                    let fans = values(&sensors["boardFans"]);
                    let temperature = values(&sensors["boardMetrics"])
                        .iter()
                        .find(|metric| text(metric, "kind").ends_with("Celsius"));
                    let summary = join(vec![
                        (!fans.is_empty())
                            .then(|| tr!("performance.cpu.fanCount", count = fans.len())),
                        temperature.map(|metric| {
                            format!(
                                "{} {} °C",
                                label(&format!(
                                    "performance.cpu.boardMetrics.{}",
                                    text(metric, "kind")
                                )),
                                reading(metric["value"].as_f64(), 1)
                            )
                        }),
                    ]);
                    pair.push(self.card(
                        label("performance.cpu.boardSensor"),
                        sensors["boardSensor"].as_str().map(String::from),
                        Some(summary),
                        false,
                        self.board_sensors(sensors),
                    ));
                }
                if !limits.is_empty() {
                    let active = limits.iter().any(|flag| boolean(flag, "active"));
                    let reasons = limits
                        .iter()
                        .filter(|flag| boolean(flag, "active"))
                        .map(|flag| {
                            label(&format!("performance.cpu.limits.{}", text(flag, "reason")))
                        })
                        .collect::<Vec<_>>();
                    let summary = if reasons.is_empty() {
                        label("performance.cpu.limitsNone")
                    } else {
                        tr!(
                            "performance.cpu.limitsActive",
                            reasons = reasons.join(&label("common.listSeparator"))
                        )
                    };
                    pair.push(self.card(
                        label("performance.cpu.packageLimits"),
                        None,
                        Some(summary),
                        active,
                        self.package_limits(limits, cx),
                    ));
                }
                // Two short cards side by side where they fit; a lone one takes the row.
                let wide = self.detail_width(window) >= 2. * 340. + 12.;
                cards.push(
                    div()
                        .grid()
                        .grid_cols(if wide && pair.len() > 1 { 2 } else { 1 })
                        .gap(px(12.))
                        .children(pair)
                        .into_any_element(),
                );
            }
        }

        let idle = values(&hw["cpuIdle"]);
        if let Some(summary) = self.idle_summary(sensors, idle) {
            cards.push(self.card(
                label("performance.cpu.idleTitle"),
                None,
                Some(summary),
                false,
                self.idle_states(sensors, idle, cx),
            ));
        }
        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .children(cards)
            .into_any_element()
    }
    fn sensor_summary(&self, sensors: &Value, package: Option<f64>) -> String {
        let temperature = sensors["controlCelsius"]
            .as_f64()
            .or_else(|| sensors["packageCelsius"].as_f64());
        join(vec![
            temperature.map(|t| {
                format!(
                    "{} {} °C",
                    label("common.resource.temperature"),
                    reading(Some(t), 0)
                )
            }),
            sensors["coreVoltage"]
                .as_f64()
                .map(|v| {
                    format!(
                        "{} {} V",
                        label("performance.cpu.coreVoltage"),
                        reading(Some(v), 3)
                    )
                })
                .or_else(|| {
                    sensors["coreVid"].as_f64().map(|v| {
                        format!(
                            "{} {} V",
                            label("performance.cpu.coreVid"),
                            reading(Some(v), 3)
                        )
                    })
                }),
            package.map(|w| {
                format!(
                    "{} {} W",
                    label("performance.cpu.packagePower"),
                    reading(Some(w), 1)
                )
            }),
        ])
    }
    /// A reading: small label, larger value with its unit.
    fn sensor_reading(&self, name: String, value: String, unit: &str) -> Div {
        div()
            .min_w_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .text_size(theme::font(11.))
                    .text_color(self.palette.muted)
                    .truncate()
                    .child(name),
            )
            .child(
                div()
                    .mt(px(2.))
                    .flex()
                    .items_baseline()
                    .gap_1()
                    .text_size(theme::font(16.))
                    .child(value)
                    .child(
                        div()
                            .text_size(theme::font(12.))
                            .text_color(self.palette.muted)
                            .child(unit.to_owned()),
                    ),
            )
    }
    /// Readings in columns at least 125 px wide.
    fn reading_grid(&self, readings: Vec<AnyElement>, width: f32) -> AnyElement {
        let columns = ((width + 18.) / (125. + 18.)).floor().max(1.) as u16;
        div()
            .grid()
            .grid_cols(columns)
            .gap_x(px(18.))
            .gap_y(px(12.))
            .children(readings)
            .into_any_element()
    }
    fn sensor_groups(&self, sensors: &Value, width: f32, cx: &mut Context<Self>) -> AnyElement {
        let p = self.palette;
        let metadata = metric_metadata();
        let metrics = values(&sensors["metrics"]);
        // The card's padding and border.
        let card_width = (width - 34.).max(200.);
        let group_columns = ((card_width + 28.) / (280. + 28.)).floor().max(1.);
        let group_width = (card_width - 28. * (group_columns - 1.)) / group_columns;
        let core_voltage = sensors["coreVoltage"].is_number()
            && !metrics.iter().any(|m| text(m, "kind") == "coreVoltage");
        let mut groups = Vec::new();
        for (group, title) in GROUPS {
            let mut readings = Vec::new();
            if group == "voltage" {
                if core_voltage {
                    readings.push(
                        self.sensor_reading(
                            label("performance.cpu.coreVoltage"),
                            reading(sensors["coreVoltage"].as_f64(), 3),
                            "V",
                        )
                        .into_any_element(),
                    );
                }
                if sensors["coreVid"].is_number() {
                    readings.push(
                        self.sensor_reading(
                            label("performance.cpu.coreVid"),
                            reading(sensors["coreVid"].as_f64(), 3),
                            "V",
                        )
                        .into_any_element(),
                    );
                }
            }
            if group == "temperature" {
                if sensors["controlCelsius"].is_number() {
                    readings.push(
                        self.sensor_reading(
                            label("performance.cpu.controlTemperature"),
                            reading(sensors["controlCelsius"].as_f64(), 0),
                            "°C",
                        )
                        .into_any_element(),
                    );
                }
                for ccd in values(&sensors["ccdTemperatures"]) {
                    readings.push(
                        self.sensor_reading(
                            tr!(
                                "performance.cpu.ccdTemperature",
                                index = ccd["index"].as_u64().unwrap_or(0) + 1
                            ),
                            reading(ccd["celsius"].as_f64(), 0),
                            "°C",
                        )
                        .into_any_element(),
                    );
                }
            }
            // Keep the metric order of the shared metadata. Core power is already
            // shown above; graphics power belongs to the GPU view.
            if let Some(kinds) = metadata.as_object() {
                for (kind, spec) in kinds {
                    if text(spec, "group") != group
                        || matches!(kind.as_str(), "coresWatts" | "graphicsWatts")
                    {
                        continue;
                    }
                    let Some(metric) = metrics.iter().find(|m| text(m, "kind") == kind) else {
                        continue;
                    };
                    let source = match text(metric, "source") {
                        "smu" => "SMU",
                        "svi2" => "SVI2",
                        "svi3" => "SVI3",
                        _ => "MSR",
                    };
                    let tip = tr!("performance.cpu.source", source = source);
                    readings.push(
                        self.sensor_reading(
                            label(&format!("performance.cpu.metrics.{kind}")),
                            reading(
                                metric["value"].as_f64(),
                                spec["digits"].as_u64().unwrap_or(1) as usize,
                            ),
                            text(spec, "unit"),
                        )
                        .id(SharedString::from(format!("cpu-metric-{kind}")))
                        .tooltip(move |window, cx| widgets::tooltip(tip.clone(), window, cx))
                        .into_any_element(),
                    );
                }
            }
            if readings.is_empty() {
                continue;
            }
            groups.push(
                div()
                    .min_w_0()
                    .child(
                        div()
                            .mb(px(12.))
                            .text_size(theme::font(13.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(label(title)),
                    )
                    .child(self.reading_grid(readings, group_width))
                    .into_any_element(),
            );
        }
        let _ = cx;
        let status = sensors["smuState"]
            .as_str()
            .filter(|state| *state != "ready")
            .map(|state| match state {
                "unsupportedTable" => tr!(
                    "performance.cpu.smuUnsupported",
                    version = format!("0x{:08X}", sensors["smuTableVersion"].as_u64().unwrap_or(0))
                ),
                "pending" => label("performance.cpu.smuPending"),
                _ => label("performance.cpu.smuUnavailable"),
            });
        div()
            .flex()
            .flex_col()
            .when_some(status, |d, status| {
                d.child(
                    div()
                        .mb(px(12.))
                        .text_size(theme::font(12.))
                        .text_color(p.muted)
                        .child(status),
                )
            })
            .child(
                div()
                    .grid()
                    .grid_cols(group_columns as u16)
                    .gap_x(px(28.))
                    .gap_y(px(20.))
                    .children(groups),
            )
            .into_any_element()
    }
    /// A compact table: 12 px, right-aligned figures under a header in the secondary ink.
    fn sensor_table(
        &self,
        id: &str,
        header: Vec<AnyElement>,
        rows: Vec<Vec<AnyElement>>,
    ) -> AnyElement {
        let p = self.palette;
        let columns = header.len() as u16;
        let cell = |content: AnyElement, first: bool, head: bool| {
            div()
                .px(px(10.))
                .py(px(7.))
                .border_b_1()
                .border_color(p.stroke)
                .whitespace_nowrap()
                .flex()
                .when(!first, |d| d.justify_end())
                .when(head, |d| d.text_color(p.muted))
                .child(content)
                .into_any_element()
        };
        let mut cells = Vec::new();
        for (index, content) in header.into_iter().enumerate() {
            cells.push(cell(content, index == 0, true));
        }
        for row in rows {
            for (index, content) in row.into_iter().enumerate() {
                cells.push(cell(content, index == 0, false));
            }
        }
        let scroll = scrollbar::keyed_handle(id);
        div()
            .id(SharedString::from(id.to_owned()))
            .track_scroll(&scroll)
            .overflow_x_scroll()
            // A vertical wheel scrolls the page, as in Chromium.
            .restrict_scroll_to_axis()
            // `overflow-x: scroll` keeps the rail even when the columns fit.
            .pb(px(scrollbar::SIZE))
            .child(
                div()
                    .min_w_full()
                    .grid()
                    .grid_cols_min_content(columns)
                    .text_size(theme::font(12.))
                    .children(cells),
            )
            .child(scrollbar::scrollbar(format!("{id}-scrollbar"), &scroll, p.dark).horizontal())
            .into_any_element()
    }
    fn head_cell(&self, name: String, unit: Option<&str>) -> AnyElement {
        div()
            .flex()
            .items_baseline()
            .gap_1()
            .child(name)
            .when_some(unit, |d, unit| {
                d.child(div().text_size(theme::font(10.)).child(unit.to_owned()))
            })
            .into_any_element()
    }
    fn core_table(&self, cores: &[Value], _cx: &mut Context<Self>) -> AnyElement {
        let p = self.palette;
        let columns = columns_of(&CORE_COLUMNS, cores);
        let hybrid = cores.iter().any(|core| boolean(core, "efficient"));
        let limit_flags = cores.iter().any(|core| !values(&core["limits"]).is_empty());
        let throttling = cores
            .iter()
            .any(|core| core["thermalThrottling"].is_boolean());
        let mut header = vec![self.head_cell(label("performance.cpu.columnCore"), None)];
        if hybrid {
            header.push(self.head_cell(label("performance.cpu.columnCoreType"), None));
        }
        for column in &columns {
            header.push(self.head_cell(label(column.label), Some(column.unit)));
        }
        if limit_flags {
            header.push(self.head_cell(label("performance.cpu.limitReasons"), None));
        } else if throttling {
            header.push(self.head_cell(label("performance.cpu.thermalThrottle"), None));
        }
        let plain = |value: String| div().child(value).into_any_element();
        let rows = cores
            .iter()
            .map(|core| {
                let mut row = vec![plain(tr!(
                    "performance.cpu.coreName",
                    index = core["core"].as_u64().unwrap_or(0) + 1
                ))];
                if hybrid {
                    row.push(plain(label(if boolean(core, "efficient") {
                        "performance.cpu.eCore"
                    } else {
                        "performance.cpu.pCore"
                    })));
                }
                for column in &columns {
                    row.push(plain(cell(core, column)));
                }
                if limit_flags {
                    let flags = values(&core["limits"]);
                    let active = flags.iter().any(|flag| boolean(flag, "active"));
                    let tip = format!(
                        "{}\n{}: {}",
                        limit_text(flags, false),
                        label("performance.cpu.limitHistory"),
                        limit_text(flags, true)
                    );
                    row.push(
                        div()
                            .id(SharedString::from(format!("core-limit-{}", core["core"])))
                            .when(active, |d| d.text_color(p.danger))
                            .child(limit_text(flags, false))
                            .tooltip(move |window, cx| widgets::tooltip(tip.clone(), window, cx))
                            .into_any_element(),
                    );
                } else if throttling {
                    let state = core["thermalThrottling"].as_bool();
                    row.push(
                        div()
                            .when(state == Some(true), |d| d.text_color(p.danger))
                            .child(match state {
                                None => missing(),
                                Some(true) => label("performance.cpu.throttleActive"),
                                Some(false) => label("performance.cpu.throttleInactive"),
                            })
                            .into_any_element(),
                    );
                }
                row
            })
            .collect();
        self.sensor_table("core-readings", header, rows)
    }
    fn package_limits(&self, limits: &[Value], _cx: &mut Context<Self>) -> AnyElement {
        let p = self.palette;
        let hint = label("performance.cpu.limitHistoryHint");
        let history_head = {
            let hint = hint.clone();
            div()
                .id("limit-history-head")
                .child(label("performance.cpu.limitHistory"))
                .tooltip(move |window, cx| widgets::tooltip(hint.clone(), window, cx))
                .into_any_element()
        };
        let header = vec![
            self.head_cell(label("performance.cpu.limitReasons"), None),
            self.head_cell(label("performance.cpu.limitCurrent"), None),
            history_head,
        ];
        let rows = limits
            .iter()
            .map(|flag| {
                let reason = text(flag, "reason");
                let hint = hint.clone();
                vec![
                    div()
                        .child(label(&format!("performance.cpu.limits.{reason}")))
                        .into_any_element(),
                    div()
                        .when(boolean(flag, "active"), |d| d.text_color(p.danger))
                        .child(label(if boolean(flag, "active") {
                            "performance.cpu.throttleActive"
                        } else {
                            "performance.cpu.throttleInactive"
                        }))
                        .into_any_element(),
                    div()
                        .id(SharedString::from(format!("limit-history-{reason}")))
                        .child(label(if boolean(flag, "occurred") {
                            "performance.cpu.limitLogged"
                        } else {
                            "performance.cpu.limitNever"
                        }))
                        .tooltip(move |window, cx| widgets::tooltip(hint.clone(), window, cx))
                        .into_any_element(),
                ]
            })
            .collect();
        self.sensor_table("package-limits", header, rows)
    }
    fn board_sensors(&self, sensors: &Value) -> AnyElement {
        let mut readings = Vec::new();
        for metric in values(&sensors["boardMetrics"]) {
            let kind = text(metric, "kind");
            let temperature = kind.ends_with("Celsius");
            readings.push(
                self.sensor_reading(
                    label(&format!("performance.cpu.boardMetrics.{kind}")),
                    reading(metric["value"].as_f64(), if temperature { 1 } else { 3 }),
                    if temperature { "°C" } else { "V" },
                )
                .into_any_element(),
            );
        }
        for fan in values(&sensors["boardFans"]) {
            readings.push(
                self.sensor_reading(
                    tr!(
                        "performance.cpu.fan",
                        index = fan["channel"].as_u64().unwrap_or(0) + 1
                    ),
                    reading(fan["rpm"].as_f64(), 0),
                    "RPM",
                )
                .into_any_element(),
            );
        }
        self.reading_grid(readings, 320.)
    }
    /// SMU residency averaged over the cores when the driver maps it, else the Windows classes.
    fn idle_summary(&self, sensors: &Value, idle: &[Value]) -> Option<String> {
        let cores = values(&sensors["cores"]);
        let smu = columns_of(&IDLE_COLUMNS[..3], cores);
        if !smu.is_empty() {
            let parts = smu
                .iter()
                .map(|column| {
                    let list = cores
                        .iter()
                        .filter_map(|core| core[column.field].as_f64())
                        .collect::<Vec<_>>();
                    Some(format!(
                        "{} {}%",
                        column.short,
                        reading(Some(list.iter().sum::<f64>() / list.len().max(1) as f64), 1)
                    ))
                })
                .collect();
            return Some(tr!(
                "performance.cpu.idleSummary",
                source = "SMU",
                values = join(parts)
            ));
        }
        let parts = OS_IDLE
            .iter()
            .map(|key| {
                os_idle(idle, key)
                    .map(|value| format!("{} {}%", key.to_uppercase(), reading(Some(value), 1)))
            })
            .collect::<Vec<_>>();
        parts.iter().any(Option::is_some).then(|| {
            tr!(
                "performance.cpu.idleSummary",
                source = "Windows",
                values = join(parts)
            )
        })
    }
    /// Windows idle classes averaged over the logical processors, then SMU residency per core.
    fn idle_states(&self, sensors: &Value, idle: &[Value], _cx: &mut Context<Self>) -> AnyElement {
        let mut body = div().flex().flex_col().gap(px(20.));
        let windows = OS_IDLE
            .iter()
            .filter_map(|key| {
                os_idle(idle, key).map(|value| {
                    self.sensor_reading(key.to_uppercase(), reading(Some(value), 1), "%")
                        .into_any_element()
                })
            })
            .collect::<Vec<_>>();
        if !windows.is_empty() {
            let source = label("performance.cpu.idleSource");
            body = body.child(
                div()
                    .id("cpu-idle-windows")
                    .min_w_0()
                    .child(
                        div()
                            .mb(px(12.))
                            .text_size(theme::font(13.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(label("performance.cpu.idleStates")),
                    )
                    .child(self.reading_grid(windows, 420.))
                    .tooltip(move |window, cx| widgets::tooltip(source.clone(), window, cx)),
            );
        }
        let cores = values(&sensors["cores"]);
        let columns = columns_of(&IDLE_COLUMNS, cores);
        if !columns.is_empty() {
            let mut header = vec![self.head_cell(label("performance.cpu.columnCore"), None)];
            for column in &columns {
                header.push(self.head_cell(label(column.label), Some("%")));
            }
            let rows = cores
                .iter()
                .map(|core| {
                    let mut row = vec![
                        div()
                            .child(tr!(
                                "performance.cpu.coreName",
                                index = core["core"].as_u64().unwrap_or(0) + 1
                            ))
                            .into_any_element(),
                    ];
                    for column in &columns {
                        row.push(div().child(cell(core, column)).into_any_element());
                    }
                    row
                })
                .collect();
            body = body.child(
                div()
                    .min_w_0()
                    .child(
                        div()
                            .mb(px(12.))
                            .text_size(theme::font(13.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(label("performance.cpu.idlePerCore")),
                    )
                    .child(self.sensor_table("idle-readings", header, rows)),
            );
        }
        body.into_any_element()
    }
}
