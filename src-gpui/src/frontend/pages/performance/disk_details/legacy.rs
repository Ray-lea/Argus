//! ATA/SCSI health cards and raw attributes, with model-specific SSD readings.
use super::*;

fn raw_hex(value: &Value) -> String {
    let Some(bytes) = value.as_array().filter(|bytes| bytes.len() == 6) else {
        return missing();
    };
    let Some(bytes) = bytes
        .iter()
        .map(|byte| byte.as_u64().filter(|byte| *byte <= 255))
        .collect::<Option<Vec<_>>>()
    else {
        return missing();
    };
    format!(
        "0x{}",
        bytes
            .iter()
            .rev()
            .map(|byte| format!("{byte:02X}"))
            .collect::<String>()
    )
}

fn ata_name(id: u64, details: &Value) -> String {
    if let Some(source) = values(&details["ataHealth"]["attributes"])
        .iter()
        .find(|source| source["id"].as_u64() == Some(id))
    {
        let key = match text(source, "metric") {
            "remainingLife" => "performance.disk.ata.lifeIndicator",
            "availableSpare" => "performance.disk.spare",
            "hostRead" => "performance.disk.totalRead",
            "hostWritten" => "performance.disk.totalWritten",
            "powerOnHours" => "performance.disk.powerOnHours",
            "powerCycles" => "performance.disk.powerCycles",
            "unsafeShutdowns" => "performance.disk.unsafeShutdowns",
            "uncorrectableErrors" => "performance.disk.uncorrectableErrors",
            "temperature" => "performance.disk.ata.temperature",
            "startStopCount" => "performance.disk.ata.startStop",
            "loadCycleCount" => "performance.disk.ata.loadCycles",
            "reallocatedSectors" => "performance.disk.ata.reallocated",
            "pendingSectors" => "performance.disk.ata.pending",
            "offlineUncorrectable" => "performance.disk.ata.offlineUncorrectable",
            "interfaceCrcErrors" => "performance.disk.ata.crc",
            _ => "performance.disk.ata.vendor",
        };
        return label(key);
    }
    let key = match id {
        1 => "readErrors",
        3 => "spinUp",
        4 => "startStop",
        5 => "reallocated",
        7 => "seekErrors",
        9 => "powerOn",
        10 => "spinRetry",
        12 => "powerCycles",
        184 => "endToEnd",
        187 => "uncorrectable",
        188 => "timeout",
        190 => "airflow",
        191 => "shock",
        192 => "retract",
        193 => "loadCycles",
        194 => "temperature",
        196 => "reallocationEvents",
        197 => "pending",
        198 => "offlineUncorrectable",
        199 => "crc",
        _ => "vendor",
    };
    label(&format!("performance.disk.ata.{key}"))
}

fn assess_legacy(health: &Value, summary: &Value) -> (&'static str, Vec<&'static str>) {
    let mut reasons = Vec::new();
    let spare_low = summary["availableSparePercent"]
        .as_u64()
        .zip(summary["spareThresholdPercent"].as_u64())
        .is_some_and(|(spare, threshold)| spare < threshold);
    if spare_low {
        reasons.push("spare");
    }
    if summary["remainingLifePercent"]
        .as_u64()
        .is_some_and(|life| life <= WORN_PERCENT)
    {
        reasons.push("worn");
    }
    if counter(&summary["uncorrectableErrors"]).is_some_and(|count| count > 0.) {
        reasons.push("uncorrectableErrors");
    }
    for (field, reason) in [
        ("reallocatedSectors", "reallocated"),
        ("pendingSectors", "pending"),
        ("offlineUncorrectable", "offlineUncorrectable"),
    ] {
        if counter(&summary[field]).is_some_and(|count| count > 0.) {
            reasons.push(reason);
        }
    }
    let state = if health["predictFailure"].as_bool() == Some(true) || spare_low {
        "bad"
    } else if !reasons.is_empty() {
        "caution"
    } else if health["predictFailure"].as_bool() == Some(false) {
        "good"
    } else {
        "unknown"
    };
    (state, reasons)
}

/// The attribute list is split into at most this many tables side by side,
/// each keeping at least this many rows.
const SPLIT_TABLES: usize = 3;
const SPLIT_ROWS: usize = 5;

fn source(health: &Value) -> String {
    let key = match text(health, "source") {
        "ata" => "ata",
        "sat" => "sat",
        "scsi" => "scsi",
        _ => "windows",
    };
    label(&format!("performance.disk.sources.{key}"))
}

impl Desktop {
    pub(super) fn disk_legacy_health(
        &self,
        disk: &Value,
        details: &Value,
        read_at: Option<String>,
        width: f32,
    ) -> AnyElement {
        let health = &details["smartHealth"];
        let summary = &details["ataHealth"];
        let (status, reasons) = assess_legacy(health, summary);
        let p = self.palette;
        let color = match status {
            "bad" => Condition::Bad.color(p),
            "caution" => Condition::Caution.color(p),
            "good" => Condition::Good.color(p),
            _ => p.muted,
        };
        let temperature = disk["temperature"]
            .as_f64()
            .filter(|value| *value != 0.)
            .or_else(|| health["temperatureCelsius"].as_f64())
            .or_else(|| summary["temperatureCelsius"].as_f64());
        let state = self
            .sensor_reading(
                label("common.status"),
                label(&format!("performance.disk.status.{status}")),
                "",
            )
            .text_color(color)
            .into_any_element();
        let state = if let Some(life) = summary["remainingLifePercent"].as_u64() {
            self.disk_tile(
                state,
                self.disk_meter(life as f64 / 100., None, color),
                Some(tr!("performance.disk.lifeLeft", percent = life)),
                None,
            )
        } else {
            state
        };
        let mut readings = vec![
            state,
            self.sensor_reading(
                label("common.resource.temperature"),
                temperature
                    .map(|v| v.round().to_string())
                    .unwrap_or_else(missing),
                "°C",
            )
            .into_any_element(),
        ];
        if let Some(spare) = summary["availableSparePercent"].as_u64() {
            let threshold = summary["spareThresholdPercent"].as_u64();
            readings.push(
                self.disk_tile(
                    self.sensor_reading(label("performance.disk.spare"), spare.to_string(), "%")
                        .into_any_element(),
                    self.disk_meter(
                        spare as f64 / 100.,
                        threshold.map(|value| value as f64 / 100.),
                        if threshold.is_some_and(|threshold| spare < threshold) {
                            p.danger
                        } else {
                            Condition::Good.color(p)
                        },
                    ),
                    threshold.map(|value| tr!("performance.disk.spareThreshold", percent = value)),
                    None,
                ),
            );
        }
        let columns = ((width + TILE_GAP) / (TILE_MIN + TILE_GAP))
            .floor()
            .max(1.)
            .min(readings.len() as f32) as u16;
        let mut body = div().flex().flex_col().gap(px(12.)).child(
            div()
                .grid()
                .grid_cols(columns)
                .gap_x(px(TILE_GAP))
                .gap_y(px(18.))
                .children(readings),
        );
        if !reasons.is_empty() {
            body = body.child(div().text_color(color).child(reason_list(&reasons)));
        }
        if summary.is_object() {
            let rotational = boolean(summary, "rotational");
            let fields = if rotational {
                vec![
                    ("powerOnHours", "powerOnHours", Some("hours")),
                    ("powerCycles", "powerCycles", None),
                    ("startStopCount", "ata.startStop", None),
                    ("loadCycleCount", "ata.loadCycles", None),
                    ("reallocatedSectors", "ata.reallocated", None),
                    ("pendingSectors", "ata.pending", None),
                    ("offlineUncorrectable", "ata.offlineUncorrectable", None),
                    ("interfaceCrcErrors", "ata.crc", None),
                    ("hostReadBytes", "totalRead", None),
                    ("hostWrittenBytes", "totalWritten", None),
                ]
            } else {
                vec![
                    ("hostReadBytes", "totalRead", None),
                    ("hostWrittenBytes", "totalWritten", None),
                    ("powerOnHours", "powerOnHours", Some("hours")),
                    ("powerCycles", "powerCycles", None),
                    ("unsafeShutdowns", "unsafeShutdowns", None),
                    ("uncorrectableErrors", "uncorrectableErrors", None),
                ]
            };
            let readings = fields
                .into_iter()
                .filter(|(field, _, _)| !rotational || !summary[*field].is_null())
                .map(|(field, title, unit)| {
                    let (value, unit) = if field.ends_with("Bytes") {
                        split(
                            counter(&summary[field])
                                .map(fmt_capacity)
                                .unwrap_or_else(missing),
                        )
                    } else {
                        (
                            grouped(&summary[field]),
                            unit.map(|unit| label(&format!("performance.disk.unit.{unit}"))),
                        )
                    };
                    self.sensor_reading(
                        label(&format!("performance.disk.{title}")),
                        value,
                        unit.as_deref().unwrap_or(""),
                    )
                    .into_any_element()
                })
                .collect();
            body = body.child(
                div()
                    .pt(px(16.))
                    .border_t_1()
                    .border_color(p.stroke)
                    .child(self.reading_grid(readings, width)),
            );
        }
        if let Some(asc) = health["scsiAsc"].as_u64().filter(|asc| *asc != 0) {
            body = body.child(div().text_color(p.danger).child(tr!(
                "performance.disk.scsi.sense",
                asc = format!("{asc:02X}"),
                ascq = format!("{:02X}", health["scsiAscq"].as_u64().unwrap_or(0))
            )));
        }
        if let Some(limit) = health["temperatureLimitCelsius"].as_f64() {
            body = body.child(
                div()
                    .text_color(p.muted)
                    .child(tr!("performance.disk.warningAt", value = limit.round())),
            );
        }
        if let Some(status) = values(&details["queries"])
            .iter()
            .find(|status| text(status, "query") == "service" && text(status, "state") != "ready")
        {
            body = body.child(self.disk_note(
                label(&format!(
                    "performance.disk.healthMissing.{}",
                    text(status, "state")
                )),
                status,
            ));
        }
        self.card(
            label("performance.disk.health"),
            Some(source(health)),
            read_at,
            status == "bad",
            body,
        )
    }

    /// The raw attributes as a table, split into several side by side where
    /// they fit, and the SCSI log counters.
    pub(super) fn disk_legacy_attributes(
        &self,
        details: &Value,
        width: f32,
        window: &Window,
        cx: &App,
    ) -> Vec<AnyElement> {
        let health = &details["smartHealth"];
        let p = self.palette;
        let mut cards = Vec::new();
        let attributes = values(&health["attributes"]);
        if !attributes.is_empty() {
            let size = theme::font(12.);
            let name_head = label("performance.disk.column.attribute");
            let heads = ["current", "worst", "threshold", "raw"]
                .map(|column| label(&format!("performance.disk.column.{column}")));
            let rows = attributes
                .iter()
                .map(|attribute| {
                    let id = attribute["id"].as_u64().unwrap_or(0);
                    (
                        format!("{id:02X}  {}", ata_name(id, details)),
                        [
                            grouped(&attribute["current"]),
                            grouped(&attribute["worst"]),
                            grouped(&attribute["threshold"]),
                            raw_hex(&attribute["raw"]),
                        ],
                        boolean(attribute, "failing"),
                    )
                })
                .collect::<Vec<_>>();
            let plain = FontWeight::NORMAL;
            let names = rows.iter().map(|(name, ..)| name.as_str());
            let name = widest(names.chain([name_head.as_str()]), size, plain, window, cx);
            let cells: [f32; 4] = std::array::from_fn(|index| {
                let column = rows.iter().map(|(_, cells, _)| cells[index].as_str());
                widest(
                    column.chain([heads[index].as_str()]),
                    size,
                    plain,
                    window,
                    cx,
                )
            });
            let most = (rows.len() / SPLIT_ROWS).clamp(1, SPLIT_TABLES);
            let sheet = Sheet::fit(width, name, &cells, most);
            let cell = |text: &str| div().child(text.to_owned()).into_any_element();
            let tables = rows
                .chunks(rows.len().div_ceil(sheet.tables))
                .map(|rows| {
                    let head = self
                        .table_row(
                            &sheet,
                            cell(&name_head),
                            heads.iter().map(|head| cell(head)).collect(),
                            false,
                        )
                        .text_color(p.muted)
                        .pt(px(0.));
                    self.table_lane(&sheet)
                        .child(head)
                        .children(rows.iter().map(|(name, figures, failing)| {
                            self.table_row(
                                &sheet,
                                cell(name),
                                figures.iter().map(|figure| cell(figure)).collect(),
                                true,
                            )
                            .when(*failing, |d| d.text_color(p.danger))
                        }))
                })
                .collect();
            let body = self.table_lanes(tables).text_size(size);
            cards.push(self.card(
                label("performance.disk.smart"),
                Some(source(health)),
                None,
                false,
                body,
            ));
        }
        let counters = values(&health["counters"]);
        if !counters.is_empty() {
            let rows = counters
                .iter()
                .map(|counter| {
                    let operation = label(if boolean(counter, "write") {
                        "performance.disk.scsi.write"
                    } else {
                        "performance.disk.scsi.read"
                    });
                    let key = match counter["parameter"].as_u64() {
                        Some(0) => "fast",
                        Some(1) => "delayed",
                        Some(2) => "retry",
                        Some(3) => "corrected",
                        Some(4) => "corrections",
                        Some(5) => "processedBytes",
                        _ => "uncorrected",
                    };
                    (
                        format!(
                            "{operation} · {}",
                            label(&format!("performance.disk.scsi.{key}"))
                        ),
                        div().child(grouped(&counter["value"])).into_any_element(),
                    )
                })
                .collect();
            cards.push(self.card(
                label("performance.disk.scsi.counters"),
                Some(source(health)),
                None,
                false,
                self.info_list(rows),
            ));
        }
        cards
    }
}

#[cfg(test)]
mod tests {
    use super::{assess_legacy, raw_hex};
    use serde_json::json;
    #[test]
    fn raw_bytes_keep_all_six_bytes_and_leading_zeroes() {
        assert_eq!(raw_hex(&json!([1, 2, 3, 4, 0, 0])), "0x000004030201");
        assert_eq!(
            raw_hex(&json!([255, 255, 255, 255, 255, 255])),
            "0xFFFFFFFFFFFF"
        );
    }

    #[test]
    fn sata_health_preserves_unknown_state_and_separates_wear_from_failure() {
        let passed = json!({"predictFailure": false});
        assert_eq!(
            assess_legacy(&passed, &json!({"remainingLifePercent": 90})).0,
            "good"
        );
        assert_eq!(
            assess_legacy(&json!({}), &json!({"remainingLifePercent": 90})).0,
            "unknown"
        );
        for life in [0, 10] {
            assert_eq!(
                assess_legacy(&passed, &json!({"remainingLifePercent": life})),
                ("caution", vec!["worn"])
            );
        }
        assert_eq!(
            assess_legacy(
                &json!({"predictFailure": true}),
                &json!({"remainingLifePercent": 90})
            )
            .0,
            "bad"
        );
        assert_eq!(
            assess_legacy(
                &passed,
                &json!({"availableSparePercent": 5, "spareThresholdPercent": 10})
            ),
            ("bad", vec!["spare"])
        );
        assert_eq!(
            assess_legacy(&passed, &json!({"uncorrectableErrors": "2"})),
            ("caution", vec!["uncorrectableErrors"])
        );
    }

    #[test]
    fn hdd_sector_warnings_do_not_turn_cable_counters_into_media_failures() {
        let passed = json!({"predictFailure":false});
        assert_eq!(
            assess_legacy(
                &passed,
                &json!({"interfaceCrcErrors":"100", "powerOnHours":"80000"})
            ),
            ("good", vec![])
        );
        let sectors =
            json!({"reallocatedSectors":"3", "pendingSectors":"2", "offlineUncorrectable":"1"});
        assert_eq!(
            assess_legacy(&passed, &sectors),
            (
                "caution",
                vec!["reallocated", "pending", "offlineUncorrectable"]
            )
        );
        assert_eq!(
            assess_legacy(&json!({"predictFailure":true}), &sectors).0,
            "bad"
        );
    }
}
