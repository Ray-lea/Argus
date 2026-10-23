use super::*;
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
            .w_56()
            .flex_shrink_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_1()
            .pr_3();
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
                window.rem_size() * 2.,
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
                .flex_col()
                .items_start()
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(device.title.clone()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(self.palette.muted)
                        .truncate()
                        .child(device.model.clone()),
                )
                .child(graph),
            );
        }
        let content = selected
            .map(|device| self.device_view(&device, window, cx))
            .unwrap_or_else(|| div().into_any_element());
        div()
            .flex_1()
            .min_h_0()
            .px_3()
            .flex()
            .child(side)
            .child(
                div()
                    .id("device-content")
                    .flex_1()
                    .min_w_0()
                    .overflow_y_scroll()
                    .child(content),
            )
            .into_any_element()
    }
    fn device_view(
        &self,
        device: &Device,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut body = div().flex().flex_col().gap_4().p_4();
        let p = self.palette;
        let sys = &self.snapshot["system"];
        let hw = &self.snapshot["hardware"];
        body = body.child(
            div()
                .flex()
                .items_center()
                .gap_4()
                .child(
                    div()
                        .text_2xl()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(device.title.clone()),
                )
                .child(
                    div()
                        .flex_1()
                        .text_right()
                        .text_color(p.muted)
                        .child(device.model.clone()),
                ),
        );
        match device.kind {
            "cpu" => {
                let cpu = &hw["cpu"];
                let controls = div().flex().gap_2().children([
                    self.button(
                        "cpu-overall",
                        label("performance.cpu.overall"),
                        None,
                        Action::CpuMode("overall".into()),
                        self.cpu_mode == "overall",
                        cx,
                    ),
                    self.button(
                        "cpu-logical",
                        label("performance.cpu.logical"),
                        None,
                        Action::CpuMode("logical".into()),
                        self.cpu_mode == "logical",
                        cx,
                    ),
                    self.button(
                        "cpu-details",
                        label("performance.cpu.page.details"),
                        None,
                        Action::CpuMode("details".into()),
                        self.cpu_page == "details",
                        cx,
                    ),
                ]);
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
                    body = body.child(self.cpu_details(hw, cx));
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
                    let em = f32::from(window.rem_size());
                    let available =
                        (f32::from(window.viewport_size().width) - em * 18.).max(em * 12.);
                    let columns = (available / (em * 12.)).floor().max(1.) as usize;
                    let mut grid = div().flex().flex_wrap().gap_3();
                    for (index, value) in core_values.iter().enumerate() {
                        if filter != "all"
                            && threads.get(index).is_some_and(|thread| {
                                boolean(thread, "efficient") != (filter == "e")
                            })
                        {
                            continue;
                        }
                        let key = format!("system.cores.{index}");
                        let chart = self.chart(
                            &format!("cpu-{index}"),
                            &[(&key, p.cpu)],
                            Some(100.),
                            window.rem_size() * 6.,
                            true,
                            cx,
                        );
                        grid = grid.child(
                            div()
                                .w(relative(1. / columns as f32))
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap_2()
                                .border_1()
                                .border_color(p.stroke)
                                .rounded(cx.theme().radius)
                                .p_3()
                                .child(
                                    div()
                                        .flex()
                                        .justify_between()
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
                        window.rem_size() * 18.,
                        true,
                        cx,
                    ));
                }
                body = body.child(div().flex().flex_wrap().children([
                    self.metric(
                        "performance.cpu.utilization",
                        format!("{}%", theme::cpu(number(sys, "cpu"))),
                    ),
                    self.metric(
                        "performance.cpu.speed",
                        optional(&hw["cpuMhz"], |v| format!("{:.2} GHz", v / 1000.)),
                    ),
                    self.metric(
                        "performance.cpu.processes",
                        theme::count(number(sys, "processCount")),
                    ),
                    self.metric(
                        "performance.cpu.threads",
                        theme::count(number(sys, "threadCount")),
                    ),
                    self.metric(
                        "performance.cpu.handles",
                        theme::count(number(sys, "handleCount")),
                    ),
                    self.metric(
                        "performance.cpu.uptime",
                        theme::uptime(number(sys, "uptimeMs") / 1000.),
                    ),
                ]));
                let cpu = &hw["cpu"];
                body = body.child(self.properties(&[
                    ("performance.cpu.sockets", cpu["sockets"].to_string()),
                    ("performance.cpu.cores", cpu["cores"].to_string()),
                    (
                        "performance.cpu.logicalProcessors",
                        cpu["logical"].to_string(),
                    ),
                    (
                        "performance.cpu.baseSpeed",
                        format!("{:.2} GHz", number(cpu, "baseMhz") / 1000.),
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
                ]));
            }
            "memory" => {
                body = body.child(self.chart(
                    "memory-usage",
                    &[("system.memUsed", p.memory)],
                    Some(number(sys, "memTotal")),
                    window.rem_size() * 18.,
                    true,
                    cx,
                ));
                body = body.child(div().flex().flex_wrap().children([
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
                ]));
                let memory = &hw["memory"];
                body = body.child(self.properties(&[
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
                        text(memory, "formFactor").into(),
                    ),
                    ("performance.memory.type", text(memory, "kind").into()),
                ]));
            }
            "disk" => {
                let disk = &hw["disks"][device.index];
                let prefix = format!("hardware.disks.{}", device.index);
                body = body
                    .child(self.chart(
                        "disk-active",
                        &[(&format!("{prefix}.active"), p.io)],
                        Some(100.),
                        window.rem_size() * 12.,
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
                        window.rem_size() * 8.,
                        true,
                        cx,
                    ));
                body = body.child(div().flex().flex_wrap().children([
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
                ]));
                body = body.child(
                    self.properties(&[
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
                    ]),
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
                    window.rem_size() * 18.,
                    true,
                    cx,
                ));
                body = body.child(div().flex().flex_wrap().children([
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
                ]));
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
                let mut grid = div().flex().flex_wrap().gap_3();
                for (index, value) in engines.iter().enumerate() {
                    let name = gpu["info"]["engines"][index].as_str().unwrap_or("");
                    let chart = self.chart(
                        &format!("gpu-engine-{index}"),
                        &[(&format!("{prefix}.engines.{index}"), p.gpu)],
                        Some(100.),
                        window.rem_size() * 8.,
                        true,
                        cx,
                    );
                    grid = grid.child(
                        div()
                            .flex_1()
                            .min_w_48()
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
                        window.rem_size() * 8.,
                        true,
                        cx,
                    ),
                );
                body = body.child(div().flex().flex_wrap().children([
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
                ]));
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
                    window.rem_size() * 14.,
                    true,
                    cx,
                ));
                body = body.child(div().flex().flex_wrap().children([
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
                ]));
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
            .gap_1()
            .children(
                items
                    .iter()
                    .map(|(key, value)| self.property(key, value.clone())),
            )
            .into_any_element()
    }
    pub(in crate::frontend) fn cpu_details(
        &self,
        hardware: &Value,
        cx: &Context<Self>,
    ) -> AnyElement {
        let sensors = &hardware["cpuSensors"];
        let mut body = div().flex().flex_col().gap_3();
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
            (
                "performance.cpu.coreVoltage",
                optional(&sensors["coreVoltage"], |v| format!("{v:.3} V")),
            ),
        ]));
        for metric in values(&sensors["metrics"]) {
            let kind = text(metric, "kind");
            let title = format!("performance.cpu.metrics.{kind}");
            static METRICS: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
            let metadata = METRICS.get_or_init(|| {
                serde_json::from_str(include_str!("../../../../resources/cpu-metrics.json"))
                    .expect("CPU metric metadata")
            });
            let unit = text(&metadata[kind], "unit");
            let digits = metadata[kind]["digits"].as_u64().unwrap_or(1) as usize;
            body = body.child(self.property(
                &title,
                optional(&metric["value"], |v| format!("{v:.digits$} {unit}")),
            ));
        }
        for temperature in values(&sensors["ccdTemperatures"]) {
            body = body.child(self.property(
                &tr!(
                    "performance.cpu.ccdTemperature",
                    index = temperature["index"]
                ),
                optional(&temperature["celsius"], |v| format!("{v:.1} °C")),
            ));
        }
        for fan in values(&sensors["boardFans"]) {
            body = body.child(self.property(
                &tr!("performance.cpu.fan", index = fan["channel"]),
                optional(&fan["rpm"], |v| format!("{v:.0} rpm")),
            ));
        }
        for metric in values(&sensors["boardMetrics"]) {
            let kind = text(metric, "kind");
            let unit = if kind.ends_with("Volts") { "V" } else { "°C" };
            body = body.child(self.property(
                &format!("performance.cpu.boardMetrics.{kind}"),
                optional(&metric["value"], |v| format!("{v:.2} {unit}")),
            ));
        }
        if !values(&sensors["limits"]).is_empty() {
            let active = values(&sensors["limits"])
                .iter()
                .filter(|limit| boolean(limit, "active"))
                .map(|limit| label(&format!("performance.cpu.limits.{}", text(limit, "reason"))))
                .collect::<Vec<_>>();
            body = body.child(self.property(
                "performance.cpu.limitReasons",
                if active.is_empty() {
                    label("performance.cpu.limitsNone")
                } else {
                    active.join(&label("common.listSeparator"))
                },
            ));
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
                    .children(
                        columns
                            .iter()
                            .map(|column| TableHead::new().child(label(text(column, "label")))),
                    ),
            );
            let rows = cores.iter().map(|core| {
                TableRow::new()
                    .child(TableCell::new().child(core["core"].to_string()))
                    .children(columns.iter().map(|column| {
                        let digits = column["digits"].as_u64().unwrap_or(0) as usize;
                        let unit = text(column, "unit");
                        TableCell::new()
                            .text_right()
                            .child(optional(&core[text(column, "field")], |value| {
                                format!("{value:.digits$} {unit}")
                            }))
                    }))
            });
            body = body
                .child(div().text_lg().child(label("performance.cpu.perCore")))
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
