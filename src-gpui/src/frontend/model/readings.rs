//! Shared live figures: values and their coverage travel together to each surface.
use super::{boolean, number, process_source, readable, source_state, values};
use argus_core::measurement::{Measurement, STALE};
use serde_json::Value;

#[derive(Clone, Copy, Debug)]
pub struct Reading {
    pub value: Option<f64>,
    pub state: Measurement,
    pub retained: bool,
}

impl Reading {
    pub fn display(self, format: impl FnOnce(f64) -> String) -> String {
        let Some(value) = self.value else {
            return crate::frontend::theme::label("format.missing");
        };
        let value = format(value);
        if self.retained {
            format!("≈ {value}")
        } else if self.state.partial {
            format!("≥ {value}")
        } else {
            value
        }
    }

    fn aggregate(readings: impl Iterator<Item = Self>, maximum: bool) -> Self {
        let mut value: Option<f64> = None;
        let mut state = Measurement {
            valid: false,
            partial: false,
            missing: 0,
        };
        let mut missing = false;
        let mut retained = false;
        let mut unavailable = false;
        let mut old_value: Option<f64> = None;
        for reading in readings {
            state.valid |= reading.state.valid;
            state.partial |= reading.state.partial;
            state.missing |= reading.state.missing;
            if let Some(v) = reading.value {
                if reading.retained {
                    old_value = Some(
                        old_value
                            .map_or(v, |before| if maximum { before.max(v) } else { before + v }),
                    );
                    missing = true;
                    continue;
                }
                value = Some(
                    value.map_or(v, |before| if maximum { before.max(v) } else { before + v }),
                );
                retained |= reading.retained;
            } else {
                missing = true;
                unavailable = true;
            }
        }
        if value.is_none() && old_value.is_some() {
            value = old_value;
            retained = true;
        }
        state.partial |= value.is_some() && if retained { unavailable } else { missing };
        if value.is_none() && state.missing == 0 {
            state = Measurement::default();
        }
        Self {
            value,
            state,
            retained,
        }
    }
}

pub fn device_reading(device: &Value, field: &str, state: &str) -> Reading {
    let value = device[field].as_f64().filter(|v| v.is_finite());
    let state = source_state(device, state).unwrap_or_else(|| {
        if value.is_some() {
            Measurement::ready()
        } else {
            Measurement::default()
        }
    });
    Reading {
        value: value.filter(|_| state.valid),
        state,
        retained: false,
    }
}

pub fn gpu_total(hardware: &Value) -> Reading {
    Reading::aggregate(
        values(&hardware["gpus"])
            .iter()
            .map(|g| device_reading(g, "util", "utilState")),
        true,
    )
}

pub fn network_total(hardware: &Value, direction: Option<&str>) -> Reading {
    let state = source_state(hardware, "networkState");
    if state.is_some_and(|s| !s.valid) {
        return Reading {
            value: None,
            state: state.unwrap(),
            retained: false,
        };
    }
    let mut total = Reading::aggregate(
        values(&hardware["nics"]).iter().map(|n| {
            if let Some(field) = direction {
                device_reading(n, field, "rateState")
            } else {
                Reading::aggregate(
                    [
                        device_reading(n, "rx", "rateState"),
                        device_reading(n, "tx", "rateState"),
                    ]
                    .into_iter(),
                    false,
                )
            }
        }),
        false,
    );
    if values(&hardware["nics"]).is_empty() && state.is_some_and(|s| s.valid) {
        total.value = Some(0.);
        total.state = state.unwrap();
    } else if let Some(state) = state {
        total.state.partial |= state.partial;
        total.state.missing |= state.missing;
    }
    total
}

pub fn process_total(processes: &[Value], key: &str) -> Reading {
    Reading::aggregate(
        processes.iter().map(|p| {
            let mut state = process_source(p, key).unwrap_or(Measurement::ready());
            let network = matches!(key, "net" | "netSent" | "netReceived");
            let io = matches!(key, "io" | "ioRead" | "ioWrite" | "ioOther");
            let valid = readable(p, key)
                && !(io && boolean(p, "countersPending"))
                && !(network && number(p, "createTime") < 0.);
            let mut retained = network && boolean(p, "netReused");
            let value = valid.then(|| match key {
                "io" => number(p, "ioRead") + number(p, "ioWrite") + number(p, "ioOther"),
                "net" => number(p, "netSent") + number(p, "netReceived"),
                _ => number(p, key),
            });
            if io && value.is_some() && (boolean(p, "stale") || boolean(p, "sampleReused")) {
                state = Measurement::missing(STALE);
                retained = true;
            }
            Reading {
                value,
                state,
                retained,
            }
        }),
        false,
    )
}

#[cfg(test)]
mod tests {
    use super::{device_reading, gpu_total, network_total, process_total};
    use serde_json::json;

    #[test]
    fn process_totals_ignore_placeholders_and_keep_coverage_and_old_values() {
        let rows = vec![
            json!({"createTime":1,"netSent":0,"netReceived":2000,"netState":{"valid":true},"ioRead":0,"ioValid":true}),
            json!({"createTime":2,"netSent":99999,"netReceived":99999,"netState":{"valid":false,"missing":2},"ioRead":99999,"ioValid":false}),
        ];
        let net = process_total(&rows, "net");
        assert_eq!(net.value, Some(2000.));
        assert!(net.state.partial);
        let io = process_total(&rows, "io");
        assert_eq!(io.value, Some(0.));
        assert!(io.state.partial);
        let mut expired = rows[1].clone();
        expired["netState"] = json!({"valid":false,"missing":4});
        assert_eq!(process_total(&[expired.clone()], "net").value, None);
        expired["netReused"] = json!(true);
        let retained = process_total(&[expired], "net");
        assert_eq!(retained.value, Some(199998.));
        assert!(retained.retained);
        let mixed = process_total(
            &[
                rows[0].clone(),
                json!({"createTime":3,"netReceived":500,"netState":{"valid":false,"missing":4},"netReused":true}),
            ],
            "net",
        );
        assert_eq!(mixed.value, Some(2000.));
        assert!(mixed.state.partial);
        assert!(!mixed.retained);
    }

    #[test]
    fn device_totals_keep_unknowns_partial_sums_and_measured_zero_distinct() {
        let mut hw = json!({"networkState":{"valid":true},"nics":[
            {"rx":0,"tx":0,"rateState":{"valid":true}},
            {"rx":99999,"tx":99999,"rateState":{"valid":false,"missing":8}}
        ],"gpus":[{"util":20,"utilState":{"valid":true}},{"util":99,"utilState":{"valid":false,"missing":2}}]});
        assert_eq!(network_total(&hw, None).value, Some(0.));
        assert!(network_total(&hw, None).state.partial);
        assert_eq!(gpu_total(&hw).value, Some(20.));
        assert!(gpu_total(&hw).state.partial);
        hw["networkState"] = json!({"valid":false,"missing":2});
        assert_eq!(network_total(&hw, None).value, None);
        hw["nics"] = json!([]);
        hw["networkState"] = json!({"valid":true});
        assert_eq!(network_total(&hw, None).value, Some(0.));
        assert_eq!(gpu_total(&json!({"gpus":[]})).value, None);
        assert_eq!(
            device_reading(
                &json!({"util":99,"utilState":{"valid":false}}),
                "util",
                "utilState"
            )
            .value,
            None
        );
    }
}
