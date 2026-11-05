//! ATA attribute tables and SCSI health fields, independent of NVMe wear data.
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

fn ata_name(id: u64) -> String {
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
        let failed = health["predictFailure"].as_bool();
        let p = self.palette;
        let (status, color) = match failed {
            Some(true) => ("bad", p.danger),
            Some(false) => ("good", p.network),
            None => ("unknown", p.muted),
        };
        let temperature = disk["temperature"]
            .as_f64()
            .or_else(|| health["temperatureCelsius"].as_f64());
        let readings = vec![
            self.sensor_reading(
                label("common.status"),
                label(&format!("performance.disk.status.{status}")),
                "",
            )
            .text_color(color)
            .into_any_element(),
            self.sensor_reading(
                label("common.resource.temperature"),
                temperature
                    .map(|v| v.round().to_string())
                    .unwrap_or_else(missing),
                "°C",
            )
            .into_any_element(),
        ];
        let mut body = div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(self.reading_grid(readings, width));
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
            failed == Some(true),
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
                                .child(format!("{id:02X}  {}", ata_name(id))),
                        )
                        .child(numbers(
                            grouped(&attribute["current"]),
                            grouped(&attribute["worst"]),
                            grouped(&attribute["threshold"]),
                            raw_hex(&attribute["raw"]),
                        )),
                );
            }
            body = body.child(
                div()
                    .mt(px(10.))
                    .text_color(p.muted)
                    .child(label("performance.disk.ata.vendorHint")),
            );
            if attributes
                .iter()
                .any(|attribute| attribute["threshold"].is_null())
            {
                body = body.child(
                    div()
                        .mt(px(4.))
                        .text_color(p.muted)
                        .child(label("performance.disk.ata.thresholdsMissing")),
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
    use super::raw_hex;
    use serde_json::json;
    #[test]
    fn raw_bytes_keep_all_six_bytes_and_leading_zeroes() {
        assert_eq!(raw_hex(&json!([1, 2, 3, 4, 0, 0])), "0x000004030201");
        assert_eq!(
            raw_hex(&json!([255, 255, 255, 255, 255, 255])),
            "0xFFFFFFFFFFFF"
        );
    }
}
