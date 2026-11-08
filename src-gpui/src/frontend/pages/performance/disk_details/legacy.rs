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
            "bad" => p.danger,
            "caution" => p.io,
            "good" => p.network,
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
                self.disk_meter(
                    life as f64 / 100.,
                    None,
                    if life <= WORN_PERCENT { p.danger } else { p.io },
                ),
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
                        p.io,
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
            let readings = [
                ("hostReadBytes", "totalRead", None),
                ("hostWrittenBytes", "totalWritten", None),
                ("powerOnHours", "powerOnHours", Some("hours")),
                ("powerCycles", "powerCycles", None),
                ("unsafeShutdowns", "unsafeShutdowns", None),
                ("uncorrectableErrors", "uncorrectableErrors", None),
            ]
            .into_iter()
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

    pub(super) fn disk_legacy_attributes(&self, details: &Value) -> Vec<AnyElement> {
        let health = &details["smartHealth"];
        let p = self.palette;
        let mut cards = Vec::new();
        let attributes = values(&health["attributes"]);
        if !attributes.is_empty() {
            let numbers = |current: String, worst: String, threshold: String, raw: String| {
                div()
                    .flex()
                    .flex_shrink_0()
                    .gap(px(8.))
                    .children(
                        [current, worst, threshold]
                            .into_iter()
                            .map(|value| div().w(theme::font(64.)).text_right().child(value)),
                    )
                    .child(div().w(px(120.)).text_right().child(raw))
            };
            let mut body = div().flex().flex_col().text_size(theme::font(12.)).child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(8.))
                    .pb(px(8.))
                    .text_color(p.muted)
                    .child(
                        div()
                            .min_w(px(160.))
                            .flex_1()
                            .child(label("performance.disk.column.attribute")),
                    )
                    .child(numbers(
                        label("performance.disk.column.current"),
                        label("performance.disk.column.worst"),
                        label("performance.disk.column.threshold"),
                        label("performance.disk.column.raw"),
                    )),
            );
            for attribute in attributes {
                let id = attribute["id"].as_u64().unwrap_or(0);
                body = body.child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap(px(8.))
                        .py(px(7.))
                        .border_t_1()
                        .border_color(p.stroke)
                        .when(boolean(attribute, "failing"), |d| d.text_color(p.danger))
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(160.))
                                .child(format!("{id:02X}  {}", ata_name(id, details))),
                        )
                        .child(numbers(
                            grouped(&attribute["current"]),
                            grouped(&attribute["worst"]),
                            grouped(&attribute["threshold"]),
                            raw_hex(&attribute["raw"]),
                        )),
                );
            }
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
}
