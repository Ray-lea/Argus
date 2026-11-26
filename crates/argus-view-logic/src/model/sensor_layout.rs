//! Remember available sensor fields, never their numeric readings. Missing
//! samples retain their layout with unknown values until sensors are disabled.
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
pub struct SensorLayout {
    cpu: Value,
    presented: Value,
    metrics: BTreeMap<String, Value>,
    cores: BTreeMap<u64, Value>,
    core_fields: BTreeSet<String>,
    fields: BTreeSet<String>,
    limits: BTreeMap<String, Value>,
}
impl SensorLayout {
    pub fn observe(&mut self, hardware: &Value) {
        let sensors = &hardware["cpuSensors"];
        if self.cpu != hardware["cpu"] || !sensors.is_object() {
            *self = Self {
                cpu: hardware["cpu"].clone(),
                ..Default::default()
            };
        }
        if !sensors.is_object() {
            return;
        }
        for field in ["coreVoltage", "coreVid", "controlCelsius"] {
            if sensors[field].is_number() {
                self.fields.insert(field.into());
            }
        }
        for metric in super::values(&sensors["metrics"]) {
            if let Some(kind) = metric["kind"].as_str() {
                self.metrics.insert(
                    kind.into(),
                    json!({"kind":kind,"source":metric["source"],"value":null}),
                );
            }
        }
        for core in super::values(&sensors["cores"]) {
            let Some(id) = core["core"].as_u64() else {
                continue;
            };
            self.cores.insert(id, json!({"core":id,"group":core["group"],"processor":core["processor"],"efficient":core["efficient"]}));
            if !super::values(&core["limits"]).is_empty() {
                self.core_fields.insert("limits".into());
            }
            if let Some(fields) = core.as_object() {
                for (field, value) in fields {
                    if (value.is_number() || value.is_boolean())
                        && !matches!(field.as_str(), "core" | "group" | "processor" | "efficient")
                    {
                        self.core_fields.insert(field.clone());
                    }
                }
            }
        }
        for limit in super::values(&sensors["limits"]) {
            if let Some(reason) = limit["reason"].as_str() {
                self.limits.insert(
                    reason.into(),
                    json!({"reason":reason,"active":null,"occurred":null}),
                );
            }
        }
        self.presented = self.present(sensors);
    }
    pub fn current(&self) -> &Value {
        &self.presented
    }
    pub fn has_field(&self, name: &str) -> bool {
        self.fields.contains(name)
    }
    pub fn present(&self, current: &Value) -> Value {
        if !current.is_object() {
            return current.clone();
        }
        let mut result = current.clone();
        let merge = |templates: Vec<Value>, values: &[Value], key: &str| {
            templates
                .into_iter()
                .map(|template| {
                    values
                        .iter()
                        .find(|value| value[key] == template[key])
                        .cloned()
                        .unwrap_or(template)
                })
                .collect::<Vec<_>>()
        };
        result["metrics"] = json!(merge(
            self.metrics.values().cloned().collect(),
            super::values(&current["metrics"]),
            "kind"
        ));
        result["limits"] = json!(merge(
            self.limits.values().cloned().collect(),
            super::values(&current["limits"]),
            "reason"
        ));
        let mut cores = merge(
            self.cores.values().cloned().collect(),
            super::values(&current["cores"]),
            "core",
        );
        for core in &mut cores {
            core["_knownFields"] = json!(self.core_fields);
        }
        result["cores"] = json!(cores);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::SensorLayout;
    use serde_json::json;
    #[test]
    fn missing_reads_preserve_fields_and_rows_without_preserving_values() {
        let mut layout = SensorLayout::default();
        let ready = json!({"cpu":{"model":191},"cpuSensors":{"state":"ready","coreVid":1.2,"metrics":[{"kind":"ringMhz","source":"msr","value":3600}],"cores":[{"core":0,"group":0,"processor":0,"clockMhz":4200}],"limits":[{"reason":"thermal","active":false,"occurred":true}]}});
        layout.observe(&ready);
        let unavailable = json!({"cpu":{"model":191},"cpuSensors":{"state":"pending"}});
        layout.observe(&unavailable);
        let shown = layout.present(&unavailable["cpuSensors"]);
        assert_eq!(shown["metrics"][0]["kind"], "ringMhz");
        assert!(shown["metrics"][0]["value"].is_null());
        assert_eq!(shown["cores"][0]["core"], 0);
        assert!(shown["cores"][0]["clockMhz"].is_null());
        assert_eq!(shown["cores"][0]["_knownFields"], json!(["clockMhz"]));
        assert!(shown["limits"][0]["active"].is_null());
        assert!(layout.has_field("coreVid"));
        layout.observe(&json!({"cpu":{"model":191},"cpuSensors":null}));
        assert!(!layout.has_field("coreVid"));
        assert!(
            layout.present(&unavailable["cpuSensors"])["metrics"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }
}
