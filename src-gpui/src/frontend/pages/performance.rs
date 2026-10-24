use super::*;
use gpui_kit::component::scroll::{ScrollableElement, ScrollbarAxis};
#[derive(Clone)]
pub(super) struct Device {
    pub(super) id: String,
    pub(super) title: String,
    pub(super) model: String,
    path: String,
    pub(super) color: Hsla,
    pub(super) kind: &'static str,
    pub(super) index: usize,
}
pub(super) fn available_devices(snapshot: &Value, p: Palette) -> Vec<Device> {
    let hw = &snapshot["hardware"];
    let mut out = vec![
        Device {
            id: "cpu".into(),
            title: label("common.resource.cpu"),
            model: text(&hw["cpu"], "name").into(),
            path: "system.cpu".into(),
            color: p.cpu,
            kind: "cpu",
            index: 0,
        },
        Device {
            id: "memory".into(),
            title: label("common.resource.memory"),
            model: theme::memory(number(&snapshot["system"], "memTotal")),
            path: "system.memUsed".into(),
            color: p.memory,
            kind: "memory",
            index: 0,
        },
    ];
    for (kind, collection, prefix, color) in [
        ("disk", "disks", "disk", p.io),
        ("network", "nics", "nic", p.network),
        ("gpu", "gpus", "gpu", p.gpu),
        ("battery", "batteries", "bat", p.battery),
    ] {
        for (index, item) in values(&hw[collection]).iter().enumerate() {
            let info = &item["info"];
            let identity = if kind == "disk" {
                info["index"].to_string()
            } else {
                text(info, "id").into()
            };
            let title = match kind {
                "disk" => format!("{} {}", label("common.resource.disk"), info["index"]),
                "gpu" => format!("{} {index}", label("common.resource.gpu")),
                "battery" => label("performance.battery.title"),
                _ => text(info, "name").into(),
            };
            let model = if kind == "disk" {
                text(info, "model")
            } else if kind == "network" {
                text(info, "description")
            } else {
                text(info, "name")
            };
            out.push(Device {
                id: format!("{prefix}:{identity}"),
                title,
                model: model.into(),
                path: format!(
                    "hardware.{collection}.{index}.{}",
                    match kind {
                        "disk" => "active",
                        "network" => "rx",
                        "gpu" => "util",
                        _ => "percent",
                    }
                ),
                color,
                kind,
                index,
            });
        }
    }
    out
}
fn optional(value: &Value, format: impl FnOnce(f64) -> String) -> String {
    value
        .as_f64()
        .map(format)
        .unwrap_or_else(|| label("format.missing"))
}
fn device_summary(device: &Device, snapshot: &Value) -> String {
    let hw = &snapshot["hardware"];
    let sys = &snapshot["system"];
    match device.kind {
        "cpu" => format!(
            "{}%  {}",
            theme::cpu(number(sys, "cpu")),
            optional(&hw["cpuMhz"], |v| format!("{:.2} GHz", v / 1000.))
        ),
        "memory" => format!(
            "{} / {}",
            theme::memory(number(sys, "memUsed")),
            theme::memory(number(sys, "memTotal"))
        ),
        "disk" => {
            let disk = &hw["disks"][device.index];
            format!(
                "{}  {}%",
                text(&disk["info"], "bus"),
                theme::cpu(number(disk, "active"))
            )
        }
        "network" => {
            let nic = &hw["nics"][device.index];
            tr!(
                "processes.vitals.netRates",
                received = theme::bytes(number(nic, "rx"), true),
                sent = theme::bytes(number(nic, "tx"), true)
            )
        }
        "gpu" => {
            let gpu = &hw["gpus"][device.index];
            format!(
                "{}%  {}",
                theme::cpu(number(gpu, "util")),
                optional(&gpu["temperature"], |v| format!("{v:.0} °C"))
            )
        }
        "battery" => format!("{:.0}%", number(&hw["batteries"][device.index], "percent")),
        _ => device.model.clone(),
    }
}
impl Desktop {
    pub(in crate::frontend) fn performance_view(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let devices = available_devices(&self.snapshot, self.palette);
        let selected = devices
            .iter()
            .find(|d| d.id == self.device)
            .or_else(|| devices.first())
            .cloned();
        let mut side = div()
            .id("devices")
            .w(px(self.perf_sidebar_width))
            .flex_shrink_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_1()
            .pr_1();
        for device in &devices {
            let graph = self.chart(
                &format!("device-preview-{}", device.id),
                &[(&device.path, device.color)],
                if device.kind == "memory" {
                    Some(number(&self.snapshot["system"], "memTotal"))
                } else if device.kind == "network" {
                    None
                } else {
                    Some(100.)
                },
                window.rem_size() * 2.5,
                false,
                cx,
            );
            side = side.child(
                self.button(
                    format!("device-{}", device.id),
                    "",
                    None,
                    Action::Device(device.id.clone()),
                    self.device == device.id,
                    cx,
                )
                .relative()
                .h_16()
                .w_full()
                .justify_start()
                .px_3()
                .gap_3()
                .when(self.device == device.id, |b| {
                    b.child(
                        div()
                            .absolute()
                            .left_0()
                            .w_1()
                            .h_6()
                            .rounded_full()
                            .bg(device.color),
                    )
                })
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .truncate()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(device.title.clone()),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(self.palette.muted)
                                .truncate()
                                .child(device_summary(device, &self.snapshot)),
                        ),
                )
                .child(
                    div()
                        .w_20()
                        .flex_shrink_0()
                        .border_1()
                        .border_color(device.color.opacity(0.35))
                        .rounded_sm()
                        .overflow_hidden()
                        .child(graph),
                ),
            );
        }
        let content = selected
            .map(|device| self.device_view(&device, window, cx))
            .unwrap_or_else(|| div().into_any_element());
        let splitter = div()
            .id("perf-sidebar-splitter")
            .w_2()
            .h_full()
            .flex_shrink_0()
            .cursor_col_resize()
            .hover(|s| s.bg(self.palette.stroke))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    this.drag = Some((
                        "perf-sidebar".into(),
                        event.position,
                        this.perf_sidebar_width,
                    ));
                    cx.notify();
                }),
            );
        div()
            .size_full()
            .min_h_0()
            .px_3()
            .flex()
            .child(side)
            .child(splitter)
            .child(
                div()
                    .id("device-content")
                    .track_scroll(&self.device_scroll)
                    .flex_1()
                    .min_w_0()
                    .border_1()
                    .border_color(self.palette.stroke)
                    .rounded_lg()
                    .bg(self.palette.layer)
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .child(content)
                    .scrollbar(&self.device_scroll, ScrollbarAxis::Vertical),
            )
            .into_any_element()
    }
    fn device_view(
        &self,
        device: &Device,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut body = div()
            .flex()
            .flex_col()
            .gap_3()
            .p_5()
            .when(!(device.kind == "cpu" && self.cpu_page == "details"), |d| {
                d.flex_1().flex_shrink_0()
            });
        let p = self.palette;
        let sys = &self.snapshot["system"];
        let hw = &self.snapshot["hardware"];
        body = body.child(
            div()
                .flex()
                .items_center()
                .flex_shrink_0()
                .gap_4()
                .child(
                    div()
                        .text_size(theme::dimension("pageTitleSize"))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(device.title.clone()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_right()
                        .text_color(p.muted)
                        .child(device.model.clone()),
                ),
        );
        match device.kind {
            "cpu" => {
                let cpu = &hw["cpu"];
                let controls = div()
                    .flex()
                    .flex_shrink_0()
                    .justify_between()
                    .gap_3()
                    .child(
                        self.segments(
                            "cpu-page",
                            ["overview", "details"]
                                .into_iter()
                                .map(|page| {
                                    (
                                        label(&format!("performance.cpu.page.{page}")),
                                        None,
                                        Action::CpuMode(if page == "details" {
                                            "details".into()
                                        } else {
                                            self.cpu_mode.clone()
                                        }),
                                        self.cpu_page == page,
                                    )
                                })
                                .collect(),
                            cx,
                        ),
                    )
                    .when(self.cpu_page != "details", |d| {
                        d.child(
                            self.segments(
                                "cpu-mode",
                                ["overall", "logical"]
                                    .into_iter()
                                    .map(|mode| {
                                        (
                                            label(&format!("performance.cpu.{mode}")),
                                            None,
                                            Action::CpuMode(mode.into()),
                                            self.cpu_mode == mode,
                                        )
                                    })
                                    .collect(),
                                cx,
                            ),
                        )
                    });
                body = body.child(controls);
                if self.cpu_page == "details" {
                    if hw["cpuSensors"]["packageCelsius"].is_number() {
                        body = body.child(self.chart(
                            "cpu-temperature",
                            &[("hardware.cpuSensors.packageCelsius", p.io)],
                            None,
                            window.rem_size() * 8.,
                            true,
                            cx,
                        ));
                    }
                    let power = if hw["cpuSensors"]["packageWatts"].is_number() {
                        "hardware.cpuSensors.packageWatts"
                    } else {
                        "hardware.power.package"
                    };
                    if hw["cpuSensors"]["packageWatts"].is_number()
                        || hw["power"]["package"].is_number()
                    {
                        body = body.child(self.chart(
                            "cpu-power",
                            &[(power, p.cpu)],
                            None,
                            window.rem_size() * 8.,
                            true,
                            cx,
                        ));
                    }
                    body = body.child(self.cpu_details(hw, window, cx));
                } else if self.cpu_mode == "logical" {
                    let core_values = values(&sys["cores"]);
                    let threads = values(&cpu["threads"]);
                    let hybrid =
                        number(cpu, "performanceCores") > 0. && number(cpu, "efficiencyCores") > 0.;
                    let filter: String = if hybrid {
                        self.prefs.get("coreFilter", "all".into())
                    } else {
                        "all".into()
                    };
                    if hybrid {
                        body = body.child(div().flex().gap_2().children(["all", "p", "e"].map(
                            |choice| {
                                self.button(
                                    format!("core-filter-{choice}"),
                                    label(&if choice == "all" {
                                        "performance.cpu.filterAll".into()
                                    } else {
                                        format!("performance.cpu.filterShort.{choice}")
                                    }),
                                    None,
                                    Action::Preference("coreFilter".into(), json!(choice)),
                                    choice == filter,
                                    cx,
                                )
                            },
                        )));
                    }
                    let visible = core_values
                        .iter()
                        .enumerate()
                        .filter(|(index, _)| {
                            filter == "all"
                                || threads.get(*index).is_none_or(|thread| {
                                    boolean(thread, "efficient") == (filter == "e")
                                })
                        })
                        .map(|(index, _)| index)
                        .collect::<Vec<_>>();
                    let side_width = if self.nav_top {
                        px(0.)
                    } else if self.nav_expanded {
                        px(192.)
                    } else {
                        px(48.)
                    };
                    let measured = self.core_grid_size.get();
                    let available = if measured.width > px(0.) {
                        measured.width
                    } else {
                        (window.viewport_size().width
                            - px(self.perf_sidebar_width)
                            - side_width
                            - px(64.))
                        .max(px(128.))
                    };
                    let gap = px(12.);
                    let columns = model::core_grid_columns(
                        visible.len(),
                        f32::from(available),
                        f32::from(measured.height),
                        f32::from(theme::dimension("coreMinWidth")),
                        f32::from(theme::dimension("coreMinHeight")),
                        f32::from(gap),
                    );
                    let rows = visible.len().div_ceil(columns).max(1);
                    let width = (available - gap * (columns - 1) as f32) / columns as f32;
                    let height = ((measured.height - gap * (rows - 1) as f32) / rows as f32)
                        .max(theme::dimension("coreMinHeight"));
                    let grid_size = self.core_grid_size.clone();
                    let owner = cx.entity().downgrade();
                    let mut grid = div()
                        .relative()
                        .w_full()
                        .flex_1()
                        .min_h((theme::dimension("coreMinHeight") + gap) * rows as f32 - gap)
                        .child(
                            canvas(
                                move |bounds, window, cx| {
                                    if grid_size.replace(bounds.size) != bounds.size {
                                        let owner = owner.clone();
                                        window.defer(cx, move |_, cx| {
                                            let _ = owner.update(cx, |_, cx| cx.notify());
                                        });
                                    }
                                },
                                |_, _, _, _| {},
                            )
                            .absolute()
                            .size_full(),
                        );
                    for (index, value) in core_values.iter().enumerate() {
                        let ordinal = visible.iter().position(|i| *i == index);
                        let key = format!("system.cores.{index}");
                        let opacity = gpui_kit::base::motion::transition(
                            format!("core-opacity-{index}"),
                            if ordinal.is_some() { 1f32 } else { 0. },
                            theme::transition(),
                            window,
                            cx,
                        );
                        if let Some(ordinal) = ordinal {
                            self.grid_bounds.borrow_mut().insert(
                                key.clone(),
                                Bounds::new(
                                    point(
                                        (width + gap) * (ordinal % columns) as f32,
                                        (height + gap) * (ordinal / columns) as f32,
                                    ),
                                    size(width, height),
                                ),
                            );
                        }
                        let Some(target) = self.grid_bounds.borrow().get(&key).copied() else {
                            continue;
                        };
                        let mut bounds = gpui_kit::base::motion::transition(
                            format!("core-bounds-{index}"),
                            target,
                            theme::transition(),
                            window,
                            cx,
                        );
                        if opacity <= 0.001 {
                            continue;
                        }
                        let initial_scale = theme::design()["layout"]["coreEnterScale"]
                            .as_f64()
                            .expect("core enter scale")
                            as f32;
                        let scale = initial_scale + (1. - initial_scale) * opacity;
                        let inset_x = bounds.size.width * ((1. - scale) / 2.);
                        let inset_y = bounds.size.height * ((1. - scale) / 2.);
                        bounds.origin += point(inset_x, inset_y);
                        bounds.size = size(bounds.size.width * scale, bounds.size.height * scale);
                        let color = if threads
                            .get(index)
                            .is_some_and(|thread| boolean(thread, "efficient"))
                        {
                            p.accent
                        } else {
                            p.cpu
                        };
                        let chart = self.chart(
                            &format!("cpu-{index}"),
                            &[(&key, color)],
                            Some(100.),
                            (bounds.size.height - px(20.)).max(px(1.)),
                            true,
                            cx,
                        );
                        grid = grid.child(
                            div()
                                .absolute()
                                .left(bounds.origin.x)
                                .top(bounds.origin.y)
                                .w(bounds.size.width)
                                .h(bounds.size.height)
                                .opacity(opacity)
                                .overflow_hidden()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .child(
                                    div()
                                        .flex()
                                        .justify_between()
                                        .text_xs()
                                        .text_color(p.muted)
                                        .child(tr!("performance.cpu.coreName", index = index))
                                        .child(optional(value, |v| format!("{}%", theme::cpu(v)))),
                                )
                                .child(chart),
                        );
                    }
                    body = body.child(grid);
                } else {
                    body = body.child(self.chart(
                        "cpu-overall-plot",
                        &[("system.cpu", p.cpu), ("system.cpuKernel", p.muted)],
                        Some(100.),
                        charts::ChartHeight::Fill,
                        true,
                        cx,
                    ));
                }
                if self.cpu_page != "details" {
                    let sensors = &hw["cpuSensors"];
                    let mut metrics = vec![
                        (
                            "performance.cpu.utilization",
                            format!("{}%", theme::cpu(number(sys, "cpu"))),
                        ),
                        (
                            "performance.cpu.speed",
                            optional(&hw["cpuMhz"], |v| format!("{:.2} GHz", v / 1000.)),
                        ),
                    ];
                    for (efficient, title) in [
                        (false, "performance.cpu.pCoreSpeed"),
                        (true, "performance.cpu.eCoreSpeed"),
                    ] {
                        let values = values(&hw["coreMhz"])
                            .iter()
                            .zip(values(&cpu["threads"]))
                            .filter(|(_, thread)| boolean(thread, "efficient") == efficient)
                            .filter_map(|(value, _)| value.as_f64())
                            .collect::<Vec<_>>();
                        if number(cpu, "efficiencyCores") > 0. && !values.is_empty() {
                            metrics.push((
                                title,
                                format!(
                                    "{:.2} GHz",
                                    values.iter().sum::<f64>() / values.len() as f64 / 1000.
                                ),
                            ));
                        }
                    }
                    if let Some(temperature) = sensors["packageCelsius"].as_f64().or_else(|| {
                        values(&sensors["coreTemperatures"])
                            .iter()
                            .filter_map(|v| v["celsius"].as_f64())
                            .max_by(f64::total_cmp)
                    }) {
                        metrics.push((
                            "common.resource.temperature",
                            format!("{temperature:.0} °C"),
                        ));
                    }
                    for (field, title) in [
                        ("package", "performance.cpu.packagePower"),
                        ("cores", "performance.cpu.corePower"),
                        ("graphics", "performance.cpu.graphicsPower"),
                    ] {
                        if let Some(value) = hw["power"][field].as_f64() {
                            metrics.push((title, format!("{value:.1} W")));
                        }
                    }
                    metrics.extend([
                        (
                            "performance.cpu.processes",
                            theme::count(number(sys, "processCount")),
                        ),
                        (
                            "performance.cpu.threads",
                            theme::count(number(sys, "threadCount")),
                        ),
                        (
                            "performance.cpu.handles",
                            theme::count(number(sys, "handleCount")),
                        ),
                        (
                            "performance.cpu.uptime",
                            theme::uptime(number(sys, "uptimeMs") / 1000.),
                        ),
                    ]);
                    let stats = div()
                        .flex_1()
                        .min_w_0()
                        .grid()
                        .grid_cols(3)
                        .gap_x_5()
                        .gap_y_2()
                        .items_start()
                        .children(
                            metrics
                                .into_iter()
                                .map(|(key, value)| self.metric(key, value)),
                        );
                    let info = self.properties(&[
                        (
                            "performance.cpu.baseSpeed",
                            format!("{:.2} GHz", number(cpu, "baseMhz") / 1000.),
                        ),
                        ("performance.cpu.sockets", cpu["sockets"].to_string()),
                        (
                            "performance.cpu.cores",
                            if number(cpu, "efficiencyCores") > 0. {
                                tr!(
                                    "performance.cpu.coresHybrid",
                                    cores = cpu["cores"],
                                    performance = cpu["performanceCores"],
                                    efficiency = cpu["efficiencyCores"]
                                )
                            } else {
                                cpu["cores"].to_string()
                            },
                        ),
                        (
                            "performance.cpu.logicalProcessors",
                            cpu["logical"].to_string(),
                        ),
                        (
                            "performance.cpu.virtualization",
                            label(if boolean(cpu, "virtualization") {
                                "common.enabled"
                            } else {
                                "performance.cpu.notEnabled"
                            }),
                        ),
                        (
                            &tr!("performance.cpu.cache", level = 1),
                            theme::bytes(number(cpu, "l1"), false),
                        ),
                        (
                            &tr!("performance.cpu.cache", level = 2),
                            theme::bytes(number(cpu, "l2"), false),
                        ),
                        (
                            &tr!("performance.cpu.cache", level = 3),
                            theme::bytes(number(cpu, "l3"), false),
                        ),
                    ]);
                    body = body.child(
                        div()
                            .flex()
                            .items_start()
                            .flex_shrink_0()
                            .gap_5()
                            .when(window.viewport_size().width < px(1050.), |d| d.flex_col())
                            .child(stats)
                            .child(div().flex_1().min_w_0().py_1().child(info)),
                    );
                }
            }
            "memory" => {
                body = body.child(self.chart(
                    "memory-usage",
                    &[("system.memUsed", p.memory)],
                    Some(number(sys, "memTotal")),
                    charts::ChartHeight::Fill,
                    true,
                    cx,
                ));
                let total = number(sys, "memTotal").max(1.);
                let used = number(sys, "memUsed");
                let lists = &sys["memoryLists"];
                let (modified, standby, free) = if lists.is_object() {
                    (
                        number(lists, "modified"),
                        number(lists, "standby"),
                        number(lists, "free"),
                    )
                } else {
                    (0., number(sys, "cached"), number(sys, "memAvailable"))
                };
                body = body.child(
                    div()
                        .flex_shrink_0()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .flex()
                                .justify_between()
                                .items_center()
                                .text_xs()
                                .text_color(p.muted)
                                .child(label("performance.memory.composition"))
                                .child(
                                    div()
                                        .flex()
                                        .gap_3()
                                        .child(tr!(
                                            "performance.memory.legendInUse",
                                            value = theme::memory(used)
                                        ))
                                        .when(modified > 0., |d| {
                                            d.child(tr!(
                                                "performance.memory.legendModified",
                                                value = theme::memory(modified)
                                            ))
                                        })
                                        .child(tr!(
                                            "performance.memory.legendStandby",
                                            value = theme::memory(standby)
                                        ))
                                        .child(tr!(
                                            "performance.memory.legendFree",
                                            value = theme::memory(free)
                                        )),
                                ),
                        )
                        .child(
                            div()
                                .w_full()
                                .h_2()
                                .rounded_full()
                                .overflow_hidden()
                                .flex()
                                .bg(p.stroke)
                                .child(
                                    div()
                                        .h_full()
                                        .w(relative((used / total).clamp(0., 1.) as f32))
                                        .bg(p.memory),
                                )
                                .when(modified > 0., |d| {
                                    d.child(
                                        div()
                                            .h_full()
                                            .w(relative((modified / total).clamp(0., 1.) as f32))
                                            .bg(p.io),
                                    )
                                })
                                .child(
                                    div()
                                        .h_full()
                                        .w(relative((standby / total).clamp(0., 1.) as f32))
                                        .bg(p.memory.opacity(0.4)),
                                ),
                        ),
                );
                let stats = div()
                    .flex_1()
                    .min_w_0()
                    .grid()
                    .grid_cols(3)
                    .gap_3()
                    .children([
                        self.metric(
                            "performance.memory.inUse",
                            theme::memory(number(sys, "memUsed")),
                        ),
                        self.metric(
                            "performance.memory.available",
                            theme::memory(number(sys, "memAvailable")),
                        ),
                        self.metric(
                            "performance.memory.committed",
                            format!(
                                "{} / {}",
                                theme::memory(number(sys, "commitUsed")),
                                theme::memory(number(sys, "commitLimit"))
                            ),
                        ),
                        self.metric(
                            "performance.memory.cached",
                            theme::memory(number(sys, "cached")),
                        ),
                        self.metric(
                            "performance.memory.pagedPool",
                            theme::memory(number(sys, "kernelPaged")),
                        ),
                        self.metric(
                            "performance.memory.nonPagedPool",
                            theme::memory(number(sys, "kernelNonpaged")),
                        ),
                    ]);
                let memory = &hw["memory"];
                let form_factor = text(memory, "formFactor");
                let reserved = (number(memory, "installed") - number(sys, "memTotal")).max(0.);
                let mut prop_items = vec![
                    (
                        "performance.memory.speed",
                        optional(&memory["speed"], |v| format!("{v:.0} MT/s")),
                    ),
                    (
                        "performance.memory.slotsUsed",
                        format!("{} / {}", memory["slotsUsed"], memory["slotsTotal"]),
                    ),
                    (
                        "performance.memory.formFactor",
                        if form_factor == "onboard" {
                            label("performance.memory.onboard")
                        } else {
                            form_factor.into()
                        },
                    ),
                    ("performance.memory.type", text(memory, "kind").into()),
                ];
                if reserved > 0. {
                    prop_items.push(("performance.memory.reserved", theme::memory(reserved)));
                }
                let specs = self.properties(&prop_items);
                body = body.child(
                    div()
                        .flex()
                        .items_start()
                        .flex_shrink_0()
                        .gap_5()
                        .when(window.viewport_size().width < px(1050.), |d| d.flex_col())
                        .child(stats)
                        .child(div().flex_1().min_w_0().py_1().child(specs)),
                );
            }
            "disk" => {
                let disk = &hw["disks"][device.index];
                let prefix = format!("hardware.disks.{}", device.index);
                body = body
                    .child(self.chart(
                        "disk-active",
                        &[(&format!("{prefix}.active"), p.io)],
                        Some(100.),
                        charts::ChartHeight::Fill,
                        true,
                        cx,
                    ))
                    .child(self.chart(
                        "disk-rate",
                        &[
                            (&format!("{prefix}.read"), p.io),
                            (&format!("{prefix}.write"), p.accent),
                        ],
                        None,
                        charts::ChartHeight::Fill,
                        true,
                        cx,
                    ));
                let stats = div()
                    .flex_1()
                    .min_w_0()
                    .grid()
                    .grid_cols(3)
                    .gap_3()
                    .children([
                        self.metric(
                            "performance.disk.activeTime",
                            format!("{:.0}%", number(disk, "active")),
                        ),
                        self.metric(
                            "performance.disk.responseTime",
                            format!("{:.2} ms", number(disk, "responseMs")),
                        ),
                        self.metric(
                            "performance.disk.readSpeed",
                            theme::bytes(number(disk, "read"), true),
                        ),
                        self.metric(
                            "performance.disk.writeSpeed",
                            theme::bytes(number(disk, "write"), true),
                        ),
                        self.metric(
                            "common.resource.temperature",
                            optional(&disk["temperature"], |v| format!("{v:.0} °C")),
                        ),
                    ]);
                let specs = self.properties(&[
                    (
                        "performance.disk.capacity",
                        theme::memory(number(&disk["info"], "size")),
                    ),
                    ("performance.disk.type", text(&disk["info"], "bus").into()),
                    (
                        "performance.disk.volumes",
                        values(&disk["info"]["volumes"])
                            .iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                            .join(", "),
                    ),
                ]);
                body = body.child(
                    div()
                        .flex()
                        .items_start()
                        .flex_shrink_0()
                        .gap_5()
                        .when(window.viewport_size().width < px(1050.), |d| d.flex_col())
                        .child(stats)
                        .child(div().flex_1().min_w_0().py_1().child(specs)),
                );
            }
            "network" => {
                let nic = &hw["nics"][device.index];
                let prefix = format!("hardware.nics.{}", device.index);
                body = body.child(self.chart(
                    "network-throughput",
                    &[
                        (&format!("{prefix}.rx"), p.network),
                        (&format!("{prefix}.tx"), p.accent),
                    ],
                    None,
                    charts::ChartHeight::Fill,
                    true,
                    cx,
                ));
                body = body.child(
                    div()
                        .flex_shrink_0()
                        .grid()
                        .grid_cols(match device.kind {
                            "memory" => 3,
                            "network" => 2,
                            "gpu" => {
                                if window.viewport_size().width < px(1100.) {
                                    3
                                } else {
                                    5
                                }
                            }
                            _ => 4,
                        })
                        .gap_3()
                        .children([
                            self.metric(
                                "performance.network.receive",
                                theme::network(
                                    number(nic, "rx"),
                                    self.prefs.get::<String>("netUnit", "bytes".into()) == "bits",
                                ),
                            ),
                            self.metric(
                                "performance.network.send",
                                theme::network(
                                    number(nic, "tx"),
                                    self.prefs.get::<String>("netUnit", "bytes".into()) == "bits",
                                ),
                            ),
                        ]),
                );
                let info = &nic["info"];
                body = body.child(
                    self.properties(&[
                        ("common.name", text(info, "description").into()),
                        (
                            "performance.network.linkSpeed",
                            format!("{:.0} Mbps", number(info, "linkSpeed") / 1e6),
                        ),
                        ("performance.network.mac", text(info, "mac").into()),
                        (
                            "performance.network.ipv4",
                            values(&info["ipv4"])
                                .iter()
                                .filter_map(Value::as_str)
                                .collect::<Vec<_>>()
                                .join("\n"),
                        ),
                        (
                            "performance.network.ipv6",
                            values(&info["ipv6"])
                                .iter()
                                .filter_map(Value::as_str)
                                .collect::<Vec<_>>()
                                .join("\n"),
                        ),
                    ]),
                );
            }
            "gpu" => {
                let gpu = &hw["gpus"][device.index];
                let prefix = format!("hardware.gpus.{}", device.index);
                let engines = values(&gpu["engines"]);
                let smart: bool = self.prefs.get("gpuEngineSmart", true);
                let visible_engines = if smart && engines.len() > 4 {
                    let mut indices = engines
                        .iter()
                        .enumerate()
                        .filter(|(_, v)| v.as_f64().unwrap_or(0.) > 0.)
                        .map(|(i, _)| i)
                        .collect::<Vec<_>>();
                    for i in 0..engines.len() {
                        if indices.len() >= 4 {
                            break;
                        }
                        if !indices.contains(&i) {
                            indices.push(i);
                        }
                    }
                    indices.truncate(4);
                    indices.sort_unstable();
                    indices
                } else {
                    (0..engines.len()).collect()
                };
                if engines.len() > 4 {
                    body = body.child(
                        div()
                            .flex()
                            .justify_end()
                            .gap_2()
                            .child(
                                self.button(
                                    "gpu-engine-smart",
                                    label("performance.gpu.engineSmart"),
                                    None,
                                    Action::Preference("gpuEngineSmart".into(), json!(true)),
                                    smart,
                                    cx,
                                )
                                .min_h_0()
                                .h_6()
                                .text_xs(),
                            )
                            .child(
                                self.button(
                                    "gpu-engine-all",
                                    tr!("performance.gpu.engineCountAll", count = engines.len()),
                                    None,
                                    Action::Preference("gpuEngineSmart".into(), json!(false)),
                                    !smart,
                                    cx,
                                )
                                .min_h_0()
                                .h_6()
                                .text_xs(),
                            ),
                    );
                }
                let rows_count = visible_engines.len().div_ceil(2).max(1);
                let mut grid = div()
                    .grid()
                    .grid_cols(2)
                    .grid_rows(rows_count as u16)
                    .gap_3()
                    .flex_1()
                    .min_h(theme::dimension("coreMinHeight") * rows_count as f32);
                for index in visible_engines {
                    let value = &engines[index];
                    let name = gpu["info"]["engines"][index].as_str().unwrap_or("");
                    let chart = self.chart(
                        &format!("gpu-engine-{index}"),
                        &[(&format!("{prefix}.engines.{index}"), p.gpu)],
                        Some(100.),
                        charts::ChartHeight::Compact,
                        true,
                        cx,
                    );
                    grid = grid.child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(format!(
                                "{name}  {}%",
                                theme::cpu(value.as_f64().unwrap_or(0.))
                            ))
                            .child(chart),
                    );
                }
                body = body.child(grid).child(
                    self.chart(
                        "gpu-memory",
                        &[
                            (&format!("{prefix}.dedicatedUsed"), p.gpu),
                            (&format!("{prefix}.sharedUsed"), p.memory),
                        ],
                        Some(
                            number(&gpu["info"], "dedicatedTotal")
                                .max(number(&gpu["info"], "sharedTotal")),
                        ),
                        window.rem_size() * 5.,
                        true,
                        cx,
                    ),
                );
                body = body.child(
                    div()
                        .flex_shrink_0()
                        .grid()
                        .grid_cols(match device.kind {
                            "memory" => 3,
                            "network" => 2,
                            "gpu" => {
                                if window.viewport_size().width < px(1100.) {
                                    3
                                } else {
                                    5
                                }
                            }
                            _ => 4,
                        })
                        .gap_3()
                        .children([
                            self.metric(
                                "performance.gpu.utilization",
                                format!("{}%", theme::cpu(number(gpu, "util"))),
                            ),
                            self.metric(
                                "performance.gpu.dedicatedMemory",
                                theme::memory(number(gpu, "dedicatedUsed")),
                            ),
                            self.metric(
                                "performance.gpu.sharedMemory",
                                theme::memory(number(gpu, "sharedUsed")),
                            ),
                            self.metric(
                                "performance.gpu.temperature",
                                optional(&gpu["temperature"], |v| format!("{v:.0} °C")),
                            ),
                            self.metric(
                                "performance.gpu.power",
                                optional(&gpu["boardPower"], |v| format!("{v:.1} W")),
                            ),
                            self.metric(
                                "performance.gpu.fan",
                                optional(&gpu["fanRpm"], theme::count),
                            ),
                            self.metric(
                                "performance.gpu.coreClock",
                                optional(&gpu["clockMhz"], |v| format!("{v:.0} MHz")),
                            ),
                            self.metric(
                                "performance.gpu.memoryClock",
                                optional(&gpu["memoryClockMhz"], |v| format!("{v:.0} MHz")),
                            ),
                            self.metric(
                                "performance.gpu.coreVoltage",
                                optional(&gpu["voltageMv"], |v| format!("{:.3} V", v / 1000.)),
                            ),
                            self.metric(
                                "performance.gpu.powerLimit",
                                optional(&gpu["boardPowerLimit"], |v| format!("{v:.1} W")),
                            ),
                        ]),
                );
                if let Some(source) = gpu["powerSource"].as_str() {
                    body = body.child(self.property(
                        "performance.gpu.powerSource",
                        label(&format!("performance.gpu.sources.{source}")),
                    ));
                }
            }
            "battery" => {
                let battery = &hw["batteries"][device.index];
                let prefix = format!("hardware.batteries.{}", device.index);
                body = body.child(self.chart(
                    "battery-level",
                    &[(&format!("{prefix}.percent"), p.battery)],
                    Some(100.),
                    charts::ChartHeight::Fill,
                    true,
                    cx,
                ));
                body = body.child(
                    div()
                        .flex_shrink_0()
                        .grid()
                        .grid_cols(match device.kind {
                            "memory" => 3,
                            "network" => 2,
                            "gpu" => {
                                if window.viewport_size().width < px(1100.) {
                                    3
                                } else {
                                    5
                                }
                            }
                            _ => 4,
                        })
                        .gap_3()
                        .children([
                            self.metric(
                                "performance.battery.level",
                                format!("{:.0}%", number(battery, "percent")),
                            ),
                            self.metric(
                                if number(battery, "rate") > 0. {
                                    "performance.battery.chargeRate"
                                } else {
                                    "performance.battery.dischargeRate"
                                },
                                optional(&battery["rate"], |v| format!("{:.2} W", v.abs() / 1000.)),
                            ),
                            self.metric(
                                "performance.battery.voltage",
                                optional(&battery["voltage"], |v| format!("{:.2} V", v / 1000.)),
                            ),
                            self.metric(
                                "common.resource.temperature",
                                optional(&battery["temperature"], |v| format!("{v:.1} °C")),
                            ),
                        ]),
                );
                let info = &battery["info"];
                body = body.child(
                    self.properties(&[
                        (
                            "common.status",
                            label(&format!(
                                "performance.battery.state.{}",
                                text(battery, "state")
                            )),
                        ),
                        (
                            "performance.battery.remaining",
                            optional(&battery["remaining"], theme::uptime),
                        ),
                        (
                            "performance.battery.health",
                            info["fullCapacity"]
                                .as_f64()
                                .zip(info["designedCapacity"].as_f64().filter(|v| *v > 0.))
                                .map(|(full, designed)| format!("{:.0}%", full / designed * 100.))
                                .unwrap_or_else(|| label("format.missing")),
                        ),
                        ("performance.battery.model", text(info, "name").into()),
                        ("performance.battery.serial", text(info, "serial").into()),
                        (
                            "performance.battery.manufactured",
                            info["manufactured"]
                                .as_str()
                                .map(String::from)
                                .unwrap_or_else(|| label("format.missing")),
                        ),
                        (
                            "performance.battery.manufacturer",
                            text(info, "manufacturer").into(),
                        ),
                        (
                            "performance.battery.chemistry",
                            text(info, "chemistry").into(),
                        ),
                        (
                            "performance.battery.designCapacity",
                            optional(&info["designedCapacity"], |v| {
                                format!("{:.1} Wh", v / 1000.)
                            }),
                        ),
                        (
                            "performance.battery.fullCapacity",
                            optional(&info["fullCapacity"], |v| format!("{:.1} Wh", v / 1000.)),
                        ),
                        (
                            "performance.battery.cycles",
                            optional(&info["cycleCount"], theme::count),
                        ),
                    ]),
                );
            }
            _ => {}
        }
        body.into_any_element()
    }
    pub(in crate::frontend) fn properties(&self, items: &[(&str, String)]) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .gap_1()
            .children(
                items
                    .iter()
                    .map(|(key, value)| self.property(key, value.clone())),
            )
            .into_any_element()
    }
    fn reading_card(
        &self,
        id: &str,
        title: &str,
        summary: Option<String>,
        readings: Vec<(String, String)>,
        cx: &Context<Self>,
    ) -> AnyElement {
        let fold_key = format!("cpu-{id}");
        let collapsed = self.sections.contains(&fold_key);
        let p = self.palette;
        div()
            .min_w_0()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .rounded_lg()
            .border_1()
            .border_color(p.stroke)
            .bg(p.layer)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        self.button(
                            format!("fold-cpu-{id}"),
                            label(title),
                            Some(if collapsed {
                                "ChevronRight"
                            } else {
                                "ChevronDown"
                            }),
                            Action::Fold(format!("s:{fold_key}")),
                            false,
                            cx,
                        )
                        .px_1()
                        .min_h_0()
                        .font_weight(FontWeight::SEMIBOLD),
                    )
                    .when_some(summary, |d, summary| {
                        d.child(div().text_xs().text_color(p.muted).child(summary))
                    }),
            )
            .when(!collapsed, |d| {
                d.children(
                    readings
                        .into_iter()
                        .map(|(key, value)| self.property(&key, value)),
                )
            })
            .into_any_element()
    }
    pub(in crate::frontend) fn cpu_details(
        &self,
        hardware: &Value,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let cpu = &hardware["cpu"];
        let sensors = &hardware["cpuSensors"];
        let sensor_state = if sensors.is_null() {
            "serviceUnavailable"
        } else {
            let s = text(sensors, "state");
            if s.is_empty() { "pending" } else { s }
        };
        let mut body = div().flex().flex_col().gap_3();
        let voltage_key = if sensors["coreVoltage"].is_number() {
            "performance.cpu.coreVoltage"
        } else if sensors["coreVid"].is_number() {
            "performance.cpu.coreVid"
        } else {
            "performance.cpu.coreVoltage"
        };
        let voltage_val = if sensors["coreVoltage"].is_number() {
            &sensors["coreVoltage"]
        } else {
            &sensors["coreVid"]
        };
        body = body.child(self.properties(&[
            (
                "common.resource.temperature",
                optional(&sensors["packageCelsius"], |v| format!("{v:.1} °C")),
            ),
            (
                "performance.cpu.packagePower",
                optional(
                    if sensors["packageWatts"].is_number() {
                        &sensors["packageWatts"]
                    } else {
                        &hardware["power"]["package"]
                    },
                    |v| format!("{v:.1} W"),
                ),
            ),
            (voltage_key, optional(voltage_val, |v| format!("{v:.3} V"))),
            (
                "performance.cpu.driverStatus",
                label(&format!("performance.cpu.sensorState.{sensor_state}")),
            ),
        ]));
        if let Some(smu) = sensors["smuState"].as_str()
            && smu != "ready"
        {
            let msg = match smu {
                "pending" => label("performance.cpu.smuPending"),
                "unsupportedTable" => tr!(
                    "performance.cpu.smuUnsupported",
                    version = format!("0x{:08X}", sensors["smuTableVersion"].as_u64().unwrap_or(0))
                ),
                _ => label("performance.cpu.smuUnavailable"),
            };
            body = body.child(
                div()
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .border_1()
                    .border_color(self.palette.stroke)
                    .text_xs()
                    .text_color(self.palette.muted)
                    .child(msg),
            );
        }
        static METRICS: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
        let metadata = METRICS.get_or_init(|| {
            serde_json::from_str(include_str!("../../../../resources/cpu-metrics.json"))
                .expect("CPU metric metadata")
        });
        let mut cards = div()
            .grid()
            .grid_cols(if window.viewport_size().width < px(1100.) {
                1
            } else {
                2
            })
            .gap_3();
        let mut info_readings = vec![
            ("performance.cpu.vendor".into(), text(cpu, "vendor").into()),
            (
                "performance.cpu.identity".into(),
                format!("{} / {} / {}", cpu["family"], cpu["model"], cpu["stepping"]),
            ),
        ];
        let inst = values(&cpu["instructions"])
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(", ");
        if !inst.is_empty() {
            info_readings.push(("performance.cpu.instructions".into(), inst));
        }
        if let Some(mc) = cpu["microcode"].as_u64() {
            info_readings.push(("performance.cpu.microcode".into(), format!("0x{mc:X}")));
        }
        cards = cards.child(self.reading_card(
            "info",
            "performance.cpu.sectionInfo",
            Some(text(cpu, "vendor").into()),
            info_readings,
            cx,
        ));
        for (group, title) in [
            ("power", "performance.cpu.telemetryPower"),
            ("voltage", "performance.cpu.telemetryVoltage"),
            ("clock", "performance.cpu.telemetryClock"),
            ("temperature", "performance.cpu.telemetryTemperature"),
        ] {
            let mut readings = Vec::new();
            for metric in values(&sensors["metrics"]) {
                let kind = text(metric, "kind");
                if text(&metadata[kind], "group") != group {
                    continue;
                }
                let digits = metadata[kind]["digits"].as_u64().unwrap_or(1) as usize;
                let unit = text(&metadata[kind], "unit");
                readings.push((
                    format!("performance.cpu.metrics.{kind}"),
                    optional(&metric["value"], |v| format!("{v:.digits$} {unit}")),
                ));
            }
            if group == "temperature" {
                for temperature in values(&sensors["ccdTemperatures"]) {
                    readings.push((
                        tr!(
                            "performance.cpu.ccdTemperature",
                            index = temperature["index"]
                        ),
                        optional(&temperature["celsius"], |v| format!("{v:.1} °C")),
                    ));
                }
                if sensors["controlCelsius"].is_number() {
                    readings.push((
                        "performance.cpu.controlTemperature".into(),
                        optional(&sensors["controlCelsius"], |v| format!("{v:.1} °C")),
                    ));
                }
                if let Some(throttling) = sensors["thermalThrottling"].as_bool() {
                    readings.push((
                        "performance.cpu.thermalThrottle".into(),
                        label(if throttling {
                            "performance.cpu.throttleActive"
                        } else {
                            "performance.cpu.throttleInactive"
                        }),
                    ));
                }
            }
            if !readings.is_empty() {
                let summary = readings.first().map(|(_, v)| v.clone());
                cards = cards.child(self.reading_card(group, title, summary, readings, cx));
            }
        }
        let mut board = vec![];
        for fan in values(&sensors["boardFans"]) {
            board.push((
                tr!("performance.cpu.fan", index = fan["channel"]),
                optional(&fan["rpm"], |v| format!("{v:.0} rpm")),
            ));
        }
        for metric in values(&sensors["boardMetrics"]) {
            let kind = text(metric, "kind");
            let unit = if kind.ends_with("Volts") { "V" } else { "°C" };
            board.push((
                format!("performance.cpu.boardMetrics.{kind}"),
                optional(&metric["value"], |v| format!("{v:.2} {unit}")),
            ));
        }
        if !board.is_empty() {
            let summary = sensors["boardSensor"]
                .as_str()
                .map(|s| tr!("performance.cpu.source", source = s));
            cards = cards.child(self.reading_card(
                "board",
                "performance.cpu.boardSensor",
                summary,
                board,
                cx,
            ));
        }
        let idle = values(&hardware["cpuIdle"]);
        if !idle.is_empty() {
            let mut idle_readings = Vec::new();
            for (field, key) in [
                ("c1", "performance.cpu.columnOsC1"),
                ("c2", "performance.cpu.columnOsC2"),
                ("c3", "performance.cpu.columnOsC3"),
            ] {
                let vals = idle
                    .iter()
                    .filter_map(|item| item[field].as_f64())
                    .collect::<Vec<_>>();
                if !vals.is_empty() {
                    let avg = vals.iter().sum::<f64>() / vals.len() as f64;
                    idle_readings.push((key.into(), format!("{avg:.1} %")));
                }
            }
            if !idle_readings.is_empty() {
                let summary = idle_readings
                    .iter()
                    .map(|(_, v)| v.as_str())
                    .collect::<Vec<_>>()
                    .join(" / ");
                cards = cards.child(self.reading_card(
                    "idle",
                    "performance.cpu.idleTitle",
                    Some(summary),
                    idle_readings,
                    cx,
                ));
            }
        }
        body = body.child(cards);
        let limits = values(&sensors["limits"]);
        if !limits.is_empty() {
            use gpui_kit::component::table::{
                Table, TableBody, TableCell, TableHead, TableHeader, TableRow,
            };
            let active = limits
                .iter()
                .filter(|limit| boolean(limit, "active"))
                .map(|limit| label(&format!("performance.cpu.limits.{}", text(limit, "reason"))))
                .collect::<Vec<_>>();
            body = body.child(self.property(
                "performance.cpu.packageLimits",
                if active.is_empty() {
                    label("performance.cpu.limitsNone")
                } else {
                    tr!(
                        "performance.cpu.limitsActive",
                        reasons = active.join(&label("common.listSeparator"))
                    )
                },
            ));
            let lim_header = TableHeader::new().child(
                TableRow::new()
                    .child(TableHead::new().child(label("performance.cpu.limitReasons")))
                    .child(TableHead::new().child(label("performance.cpu.limitCurrent")))
                    .child(TableHead::new().child(label("performance.cpu.limitHistory"))),
            );
            let lim_rows = limits.iter().map(|limit| {
                let reason = label(&format!("performance.cpu.limits.{}", text(limit, "reason")));
                let cur = if boolean(limit, "active") {
                    label("performance.cpu.throttleActive")
                } else {
                    label("performance.cpu.throttleInactive")
                };
                let hist = if boolean(limit, "occurred") {
                    label("performance.cpu.limitLogged")
                } else {
                    label("performance.cpu.limitNever")
                };
                TableRow::new()
                    .child(TableCell::new().child(reason))
                    .child(TableCell::new().child(cur))
                    .child(TableCell::new().child(hist))
            });
            body = body.child(
                div().id("package-limits-table").overflow_x_scroll().child(
                    Table::new()
                        .child(lim_header)
                        .child(TableBody::new().children(lim_rows)),
                ),
            );
        }
        let cores = values(&sensors["cores"]);
        if !cores.is_empty() {
            use gpui_kit::component::table::{
                Table, TableBody, TableCell, TableHead, TableHeader, TableRow,
            };
            static COLUMNS: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
            let definitions = COLUMNS.get_or_init(|| {
                serde_json::from_str(include_str!("../../../../resources/core-telemetry.json"))
                    .expect("core telemetry metadata")
            });
            let hybrid =
                number(cpu, "performanceCores") > 0. && number(cpu, "efficiencyCores") > 0.;
            let has_throttle = cores
                .iter()
                .any(|core| core["thermalThrottling"].is_boolean());
            let columns = values(definitions)
                .iter()
                .filter(|column| {
                    cores
                        .iter()
                        .any(|core| core[text(column, "field")].is_number())
                })
                .collect::<Vec<_>>();
            let header = TableHeader::new().child(
                TableRow::new()
                    .child(TableHead::new().child(label("performance.cpu.columnCore")))
                    .when(hybrid, |r| {
                        r.child(TableHead::new().child(label("performance.cpu.columnCoreType")))
                    })
                    .children(
                        columns
                            .iter()
                            .map(|column| TableHead::new().child(label(text(column, "label")))),
                    )
                    .when(has_throttle, |r| {
                        r.child(TableHead::new().child(label("performance.cpu.thermalThrottle")))
                    }),
            );
            let rows = cores.iter().map(|core| {
                TableRow::new()
                    .child(TableCell::new().child(core["core"].to_string()))
                    .when(hybrid, |r| {
                        r.child(TableCell::new().child(label(if boolean(core, "efficient") {
                            "performance.cpu.eCore"
                        } else {
                            "performance.cpu.pCore"
                        })))
                    })
                    .children(columns.iter().map(|column| {
                        let digits = column["digits"].as_u64().unwrap_or(0) as usize;
                        let unit = text(column, "unit");
                        TableCell::new()
                            .text_right()
                            .child(optional(&core[text(column, "field")], |value| {
                                format!("{value:.digits$} {unit}")
                            }))
                    }))
                    .when(has_throttle, |r| {
                        r.child(TableCell::new().child(
                            if core["thermalThrottling"].as_bool() == Some(true) {
                                label("performance.cpu.throttleActive")
                            } else {
                                label("performance.cpu.throttleInactive")
                            },
                        ))
                    })
            });
            body = body
                .child(
                    div()
                        .flex()
                        .justify_between()
                        .items_center()
                        .child(div().text_lg().child(label("performance.cpu.perCore")))
                        .child(
                            div()
                                .text_xs()
                                .text_color(self.palette.muted)
                                .child(tr!("performance.cpu.coreCount", count = cores.len())),
                        ),
                )
                .child(
                    div().id("core-readings").overflow_x_scroll().child(
                        Table::new()
                            .child(header)
                            .child(TableBody::new().children(rows)),
                    ),
                );
        }
        if sensors.is_null() || text(sensors, "state") != "ready" {
            body = body.child(self.button(
                "sensor-settings",
                tr!("common.privilegedService"),
                None,
                Action::Page("settings".into()),
                false,
                cx,
            ));
        }
        body.into_any_element()
    }
}
