//! Issue-report text assembled on demand from already available UI data.
//! Keep the field allowlist explicit: settings and snapshots contain private data.
use super::{Desktop, model::values, theme, theme::label, widgets};
use gpui_kit::{App, Window};
use serde_json::Value;
use std::collections::HashMap;

struct ViewState {
    display: Option<(f32, f32)>,
    viewport: (f32, f32),
    scale: f32,
    ui_scale: u32,
    font_size: u32,
    interval: u64,
    paused: bool,
}

impl Desktop {
    pub(in crate::frontend) fn diagnostic_summary(&self, window: &Window, cx: &App) -> String {
        let size = window.viewport_size();
        let view = ViewState {
            display: window.display(cx).map(|display| {
                let size = display.bounds().size;
                (size.width.into(), size.height.into())
            }),
            viewport: (size.width.into(), size.height.into()),
            scale: window.scale_factor(),
            ui_scale: self.ui_scale,
            font_size: self.font_size,
            interval: self.interval,
            paused: self.paused,
        };
        summary(&self.data, &self.snapshot, &view)
    }
}

fn unknown() -> String {
    label("settings.about.diagnostics.unknown")
}

/// Metadata may contain line breaks or control characters. Keep each value on
/// one line, bound its size, and prevent it from closing the Markdown code block.
fn clean(value: &str) -> String {
    value
        .chars()
        .take(256)
        .map(|c| {
            if c.is_control() || matches!(c, '\u{2028}'..='\u{202e}' | '\u{2066}'..='\u{2069}') {
                ' '
            } else if c == '`' {
                '\''
            } else {
                c
            }
        })
        .collect::<String>()
        .trim()
        .to_owned()
}

fn string(value: &Value) -> String {
    value
        .as_str()
        .map(clean)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(unknown)
}

fn switch(value: Option<bool>) -> String {
    value
        .map(|on| {
            label(if on {
                "common.enabled"
            } else {
                "common.disabled"
            })
        })
        .unwrap_or_else(unknown)
}

fn dimension(size: (f32, f32), scale: f32) -> String {
    let (width, height) = (size.0 * scale, size.1 * scale);
    if width.is_finite() && height.is_finite() && width > 0. && height > 0. {
        format!("{width:.0} × {height:.0} px")
    } else {
        unknown()
    }
}

fn milliseconds(value: &Value) -> String {
    value
        .as_u64()
        .filter(|&ms| ms > 0)
        .map(|ms| format!("{ms} ms"))
        .unwrap_or_else(unknown)
}

fn summary(data: &HashMap<String, Value>, snapshot: &Value, view: &ViewState) -> String {
    let info = data.get("info").unwrap_or(&Value::Null);
    let settings = data.get("settings").unwrap_or(&Value::Null);
    let hardware = &snapshot["hardware"];
    let mut lines = Vec::new();
    let mut row = |key: &str, value: String| {
        lines.push(argus_core::tr!(
            "settings.about.diagnostics.row",
            name = label(key),
            value = clean(&value)
        ));
    };
    let build = info["build"]
        .as_str()
        .filter(|build| !build.trim().is_empty())
        .map(clean)
        .unwrap_or_else(|| string(&info["version"]));
    row("settings.about.diagnostics.version", build);
    row(
        "settings.about.diagnostics.windows",
        string(&info["windows"]),
    );
    row(
        "settings.about.diagnostics.build",
        match info["buildKind"].as_str() {
            Some("development") => label("common.build.development"),
            Some("test") => label("common.build.test"),
            Some("stable") => label("common.build.stable"),
            _ => unknown(),
        },
    );
    row(
        "settings.about.diagnostics.installation",
        if info["buildKind"] == "development" {
            label("settings.about.diagnostics.source")
        } else {
            info["portable"]
                .as_bool()
                .map(|portable| {
                    label(if portable {
                        "settings.about.diagnostics.portable"
                    } else {
                        "settings.about.diagnostics.installed"
                    })
                })
                .unwrap_or_else(unknown)
        },
    );
    row(
        "settings.about.diagnostics.permission",
        info["elevated"]
            .as_bool()
            .map(|elevated| {
                label(if elevated {
                    "settings.about.diagnostics.administrator"
                } else {
                    "settings.about.diagnostics.standard"
                })
            })
            .unwrap_or_else(unknown),
    );
    row(
        "common.privilegedService",
        widgets::service_state(data.get("privilege").unwrap_or(&Value::Null)),
    );
    row(
        "settings.taskManager.title",
        match data.get("taskmgr").and_then(|v| v["state"].as_str()) {
            Some(state @ ("enabled" | "disabled" | "conflict" | "recoveryNeeded")) => {
                label(&format!("settings.taskManager.state.{state}"))
            }
            _ => unknown(),
        },
    );
    row(
        "settings.sensors.title",
        switch(settings["kernelSensors"].as_bool()),
    );
    row(
        "settings.about.diagnostics.sensorReadings",
        match hardware["cpuSensors"]["state"].as_str() {
            Some(
                state @ ("pending" | "serviceUnavailable" | "driverUnavailable" | "moduleRejected"
                | "unsupported" | "noData" | "ready"),
            ) => label(&format!("performance.cpu.sensorState.{state}")),
            _ => unknown(),
        },
    );
    let timestamp = snapshot["timestamp"]
        .as_i64()
        .filter(|&t| t > 0)
        .and_then(|ms| {
            time::OffsetDateTime::from_unix_timestamp_nanos(i128::from(ms) * 1_000_000).ok()
        })
        .map(|t| t.to_string())
        .unwrap_or_else(unknown);
    row("settings.about.diagnostics.sampleTime", timestamp);
    row("common.resource.cpu", string(&hardware["cpu"]["name"]));
    row(
        "settings.about.diagnostics.cpuCores",
        match (
            hardware["cpu"]["cores"].as_u64(),
            hardware["cpu"]["logical"].as_u64(),
        ) {
            (Some(cores), Some(logical)) if cores > 0 && logical > 0 => {
                format!("{cores} / {logical}")
            }
            _ => unknown(),
        },
    );
    let memory = hardware["memory"]["installed"]
        .as_u64()
        .filter(|&n| n > 0)
        .or_else(|| snapshot["system"]["memTotal"].as_u64().filter(|&n| n > 0));
    row(
        "common.resource.memory",
        memory
            .map(|n| theme::bytes(n as f64, false))
            .unwrap_or_else(unknown),
    );
    let gpus = values(&hardware["gpus"]);
    if gpus.is_empty() {
        row("common.resource.gpu", unknown());
    }
    for gpu in gpus.iter().take(32) {
        row("common.resource.gpu", string(&gpu["info"]["name"]));
        row(
            "performance.gpu.driverVersion",
            string(&gpu["info"]["driverVersion"]),
        );
    }
    for disk in values(&hardware["disks"]).iter().take(32) {
        // Disk model and bus are useful for compatibility reports; omit serials,
        // volume labels, drive letters and device paths.
        row(
            "common.resource.disk",
            format!(
                "{} ({})",
                string(&disk["info"]["model"]),
                string(&disk["info"]["bus"])
            ),
        );
    }
    // Window scale includes Argus's content zoom; monitor bounds do not.
    let system_scale = view.scale / (view.ui_scale as f32 / 100.);
    row(
        "settings.about.diagnostics.display",
        view.display
            .map(|size| dimension(size, system_scale))
            .unwrap_or_else(unknown),
    );
    row(
        "settings.about.diagnostics.window",
        dimension(view.viewport, view.scale),
    );
    row(
        "settings.about.diagnostics.systemScale",
        if system_scale.is_finite() && system_scale > 0. {
            format!("{:.0}%", system_scale * 100.)
        } else {
            unknown()
        },
    );
    row(
        "settings.appearance.scaleTitle",
        format!("{}%", view.ui_scale),
    );
    row(
        "settings.about.diagnostics.fontSize",
        format!("{} px", view.font_size),
    );
    row(
        "settings.about.diagnostics.language",
        argus_core::i18n::locale().tag().into(),
    );
    row(
        "settings.about.diagnostics.refresh",
        format!("{} ms", view.interval),
    );
    row(
        "settings.data.minimized.title",
        milliseconds(&settings["minimizedInterval"]),
    );
    row(
        "settings.data.tray.title",
        milliseconds(&settings["trayInterval"]),
    );
    row(
        "settings.about.diagnostics.paused",
        switch(Some(view.paused)),
    );
    format!(
        "{}\n\n```text\n{}\n```",
        label("settings.about.diagnostics.title"),
        lines.join("\n")
    )
}

#[cfg(test)]
mod tests {
    use super::{ViewState, clean, dimension, summary, switch, unknown};
    use serde_json::{Value, json};
    use std::collections::HashMap;

    fn view() -> ViewState {
        ViewState {
            display: Some((1920., 1080.)),
            viewport: (800., 600.),
            scale: 1.875,
            ui_scale: 150,
            font_size: 13,
            interval: 500,
            paused: false,
        }
    }

    #[test]
    fn report_uses_current_values_and_omits_private_snapshot_and_settings_fields() {
        crate::catalog::register();
        let data = HashMap::from([
            (
                "info".into(),
                json!({"version":"0.5.0", "build":"0.5.0+800.g123abcd", "windows":"Windows 11 24H2 (26100.4061)", "elevated":false}),
            ),
            (
                "settings".into(),
                json!({"kernelSensors":false, "minimizedInterval":10000, "trayInterval":5000, "updateProxy":"private-proxy", "rules":["private-rule"]}),
            ),
            (
                "taskmgr".into(),
                json!({"state":"conflict", "detail":"private-error", "recoveryPath":"private-path"}),
            ),
        ]);
        let mut snapshot = json!({"hardware":{"cpu":{"name":"Example CPU", "cores":14, "logical":20}, "gpus":[{"info":{"name":"Example GPU", "driverVersion":"610.88", "id":"private-id"}}], "disks":[{"info":{"model":"Example SSD", "bus":"nvme", "serial":"private-serial", "volumes":["private-volume"]}}], "nics":[{"name":"private-network"}]}, "processes":[{"name":"private-process", "commandLine":"private-command"}], "sessions":[{"name":"private-account"}]});
        let first = summary(&data, &snapshot, &view());
        for expected in [
            "0.5.0+800.g123abcd",
            "26100.4061",
            "Example CPU",
            "Example GPU",
            "610.88",
            "Example SSD",
            "14 / 20",
            "2400 × 1350 px",
            "1500 × 1125 px",
            "125%",
            "150%",
            "10000 ms",
        ] {
            assert!(first.contains(expected), "{expected}");
        }
        assert!(!first.contains("private-"));
        snapshot["hardware"]["cpu"]["name"] = json!("Changed CPU");
        let second = summary(&data, &snapshot, &view());
        assert!(second.contains("Changed CPU"));
        assert!(!second.contains("Example CPU"));
    }

    #[test]
    fn missing_states_stay_unknown_instead_of_becoming_disabled_or_zero() {
        crate::catalog::register();
        assert_eq!(switch(None), unknown());
        assert_ne!(switch(Some(false)), unknown());
        assert_eq!(dimension((0., 100.), 1.), unknown());
        let report = summary(&HashMap::new(), &Value::Null, &view());
        assert!(report.contains(&unknown()));
        assert!(!report.contains("0 / 0"));
        assert!(!report.contains("0 B"));
        assert!(!report.contains("1970-"));
    }

    #[test]
    fn metadata_cannot_inject_report_lines_or_markdown_fences() {
        crate::catalog::register();
        assert_eq!(clean("  CPU\n\r\t```\u{202e}X  "), "CPU   ''' X");
        assert_eq!(clean(&"x".repeat(512)).len(), 256);
        let snapshot = json!({"hardware":{"cpu":{"name":"CPU\n```\nInjected"}}});
        let report = summary(&HashMap::new(), &snapshot, &view());
        assert_eq!(report.matches("```").count(), 2);
        assert!(!report.contains("\nInjected"));
    }
}
