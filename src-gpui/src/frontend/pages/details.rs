use super::*;

impl Desktop {
    pub(in crate::frontend) fn details_view(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = self
            .selected
            .get("processes")
            .and_then(|key| self.rows.iter().find(|row| &row.key == key));
        let mut body = div()
            .id("process-details")
            .w(px(self.details_width))
            .min_w_48()
            .flex_shrink_0()
            .overflow_y_scroll()
            .p_4()
            .flex()
            .flex_col()
            .gap_3();
        if let Some(row) = selected.filter(|r| r.section.is_none()) {
            let info = self
                .data
                .get(&format!("details:{}", row.key))
                .cloned()
                .unwrap_or(Value::Null);
            body = body.child(
                div()
                    .text_lg()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(row.title.clone()),
            );
            if row.members.len() > 1 {
                body = body.child(tr!("details.group.count", count = row.members.len()));
            } else {
                body = body
                    .child(self.property("processes.column.pid.label", row.raw["pid"].to_string()));
            }
            let sum = |field: &str| row.members.iter().map(|p| number(p, field)).sum::<f64>();
            for (key, value) in [
                (
                    "common.resource.cpu",
                    format!("{}%", theme::cpu(sum("cpu"))),
                ),
                ("details.privateWs", theme::memory(sum("privateWs"))),
                ("details.workingSet", theme::memory(sum("workingSet"))),
                ("details.commit", theme::memory(sum("commit"))),
                ("details.cpuTime", theme::uptime(sum("cpuTime") / 1000.)),
                ("details.gpuDedicated", theme::memory(sum("gpuDedicated"))),
                ("details.gpuShared", theme::memory(sum("gpuShared"))),
                ("details.threads", theme::count(sum("threads"))),
                ("details.handles", theme::count(sum("handles"))),
            ] {
                body = body.child(self.property(key, value));
            }
            if let Some(started) = row
                .members
                .iter()
                .filter_map(|p| p["createTime"].as_i64())
                .min()
            {
                body = body.child(self.property(
                    if row.members.len() > 1 {
                        "details.group.earliest"
                    } else {
                        "details.started"
                    },
                    theme::timestamp(started),
                ));
            }
            if row.members.len() == 1 {
                body =
                    body.child(self.property("details.session", row.raw["sessionId"].to_string()));
                if let Some(parent) = values(&self.snapshot["processes"]).iter().find(|p| {
                    p["pid"] == row.raw["ppid"]
                        && number(p, "createTime") <= number(&row.raw, "createTime")
                }) {
                    body = body.child(self.button(
                        "parent-process",
                        tr!(
                            "details.parentLink",
                            name = model::title(parent),
                            pid = parent["pid"]
                        ),
                        None,
                        Action::Preference("revealProcess".into(), parent.clone()),
                        false,
                        cx,
                    ));
                }
            }
            let prefix = if row.members.len() > 1 {
                format!("group.{}", row.key)
            } else {
                format!("process.{}", row.key)
            };
            body = body
                .child(self.chart(
                    "selected-cpu",
                    &[(&format!("{prefix}.cpu"), self.palette.cpu)],
                    Some(100.),
                    window.rem_size() * 5.,
                    true,
                    cx,
                ))
                .child(self.chart(
                    "selected-memory",
                    &[(&format!("{prefix}.mem"), self.palette.memory)],
                    None,
                    window.rem_size() * 5.,
                    true,
                    cx,
                ));
            for (key, field) in [
                ("details.user", "user"),
                ("details.section.commandLine", "commandLine"),
            ] {
                if let Some(value) = info[field].as_str() {
                    body = body
                        .child(self.property(key, value.to_owned()))
                        .child(self.button(
                            format!("copy-{field}"),
                            label("common.copyName"),
                            None,
                            Action::Copy(value.into()),
                            false,
                            cx,
                        ));
                }
            }
            for (key, field, namespace) in [
                ("details.integrity", "integrity", "details.integrityLevel"),
                ("details.architecture", "architecture", "details.arch"),
                ("details.priority", "priorityClass", "processes.priority"),
            ] {
                if let Some(value) = info[field].as_str() {
                    body = body.child(self.property(key, label(&format!("{namespace}.{value}"))));
                }
            }
            for (key, field) in [
                ("details.product", "productName"),
                ("details.version", "fileVersion"),
            ] {
                if let Some(value) = info["version"][field].as_str() {
                    body = body.child(self.property(key, value.to_owned()));
                }
            }
            if let Some(path) = row.raw["path"].as_str() {
                body = body
                    .child(self.property("common.path", path.to_owned()))
                    .child(self.button(
                        "reveal-process",
                        label("common.openFileLocation"),
                        None,
                        Action::Invoke("reveal_in_explorer".into(), json!({"path":path})),
                        false,
                        cx,
                    ));
            }
            for member in &row.members {
                if row.members.len() > 1 {
                    body = body.child(self.button(
                        format!("member-{}", model::identity(member)),
                        format!("{} · {}", model::title(member), member["pid"]),
                        None,
                        Action::Preference("revealProcess".into(), member.clone()),
                        false,
                        cx,
                    ));
                }
            }
            body = body.child(self.button(
                "end-selected",
                label("common.endProcess"),
                None,
                Action::Dialog(
                    "end".into(),
                    json!({"members":row.members,"name":row.title}),
                ),
                false,
                cx,
            ));
        } else if self.selected.contains_key("processes") {
            body = body
                .child(label("details.exited.title"))
                .child(label("details.exited.hint"));
        } else {
            body = body.child(self.overview_view(cx));
        }
        div()
            .flex()
            .border_l_1()
            .border_color(self.palette.stroke)
            .child(
                div()
                    .id("details-splitter")
                    .w_1()
                    .cursor_col_resize()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &MouseDownEvent, _, cx| {
                            this.drag =
                                Some(("details".into(), event.position, this.details_width));
                            cx.stop_propagation();
                        }),
                    ),
            )
            .child(body)
            .into_any_element()
    }
    fn overview_view(&self, cx: &Context<Self>) -> AnyElement {
        let devices = super::performance::available_devices(&self.snapshot, self.palette);
        let hardware = &self.snapshot["hardware"];
        let visible = self.prefs.get("overviewHardwareVisible", true);
        let copied = devices
            .iter()
            .map(|d| {
                tr!(
                    "details.overview.copyPart",
                    kind = &d.title,
                    name = &d.model
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let mut body = div()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_xl()
                            .flex_1()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(label("details.overview.title")),
                    )
                    .child(self.button(
                        "copy-hardware",
                        label("details.overview.copy"),
                        None,
                        Action::Copy(copied),
                        false,
                        cx,
                    )),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(self.palette.muted)
                    .child(label("details.overview.selectTip")),
            )
            .child(self.button(
                "hardware-visible",
                label(if visible {
                    "details.overview.hide"
                } else {
                    "details.overview.show"
                }),
                None,
                Action::Preference("overviewHardwareVisible".into(), json!(!visible)),
                false,
                cx,
            ));
        if visible {
            for device in devices {
                let detail = match device.kind {
                    "cpu" => [
                        tr!("details.overview.cores", count = hardware["cpu"]["cores"]),
                        tr!(
                            "details.overview.threads",
                            count = hardware["cpu"]["logical"]
                        ),
                        tr!(
                            "details.overview.base",
                            speed =
                                format!("{:.2} GHz", number(&hardware["cpu"], "baseMhz") / 1000.)
                        ),
                    ]
                    .join(" · "),
                    "memory" => [
                        text(&hardware["memory"], "kind").to_owned(),
                        tr!(
                            "details.overview.slots",
                            used = hardware["memory"]["slotsUsed"],
                            total = hardware["memory"]["slotsTotal"]
                        ),
                    ]
                    .join(" · "),
                    "disk" => {
                        let info = &hardware["disks"][device.index]["info"];
                        format!(
                            "{} · {}",
                            theme::bytes(number(info, "size"), false),
                            values(&info["volumes"])
                                .iter()
                                .filter_map(Value::as_str)
                                .collect::<Vec<_>>()
                                .join(" ")
                        )
                    }
                    "gpu" => tr!(
                        "details.overview.vram",
                        size = theme::memory(number(
                            &hardware["gpus"][device.index]["info"],
                            "dedicatedTotal"
                        ))
                    ),
                    "network" => {
                        let info = &hardware["nics"][device.index]["info"];
                        if text(info, "kind") == "virtual" {
                            continue;
                        }
                        format!(
                            "{} · {:.0} Mbps",
                            text(info, "name"),
                            number(info, "linkSpeed") / 1e6
                        )
                    }
                    "battery" => {
                        text(&hardware["batteries"][device.index]["info"], "manufacturer").into()
                    }
                    _ => String::new(),
                };
                let glyph = match device.kind {
                    "cpu" => "CpuIcon",
                    "memory" => "MemoryIcon",
                    "disk" => "DiskIcon",
                    "network" => "GlobeIcon",
                    "gpu" => "GpuIcon",
                    _ => "BatteryIcon",
                };
                body = body.child(
                    self.button(
                        format!("overview-{}", device.id),
                        "",
                        None,
                        Action::Device(device.id),
                        false,
                        cx,
                    )
                    .w_full()
                    .items_start()
                    .justify_start()
                    .px_0()
                    .child(icon(glyph, device.color))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(self.palette.muted)
                                    .child(device.title),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(device.model),
                            )
                            .child(div().text_xs().text_color(self.palette.muted).child(detail)),
                    ),
                );
            }
        }
        body.into_any_element()
    }
}
