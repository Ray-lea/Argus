//! Observable per-process metrics for the process observation page.
use super::super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MetricKind {
    Bytes,
    Percent,
    Rate,
    NetRate,
    Count,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct WatchMetric {
    pub id: &'static str,
    pub kind: MetricKind,
    pub primary: &'static str,
    pub secondary: Option<&'static str>,
}

pub(super) const METRICS: [WatchMetric; 11] = [
    WatchMetric {
        id: "memory",
        kind: MetricKind::Bytes,
        primary: "memory",
        secondary: None,
    },
    WatchMetric {
        id: "commit",
        kind: MetricKind::Bytes,
        primary: "commit",
        secondary: None,
    },
    WatchMetric {
        id: "workingSet",
        kind: MetricKind::Bytes,
        primary: "workingSet",
        secondary: None,
    },
    WatchMetric {
        id: "cpu",
        kind: MetricKind::Percent,
        primary: "cpu",
        secondary: None,
    },
    WatchMetric {
        id: "gpu",
        kind: MetricKind::Percent,
        primary: "gpu",
        secondary: None,
    },
    WatchMetric {
        id: "gpuMemory",
        kind: MetricKind::Bytes,
        primary: "gpuMemory",
        secondary: None,
    },
    WatchMetric {
        id: "disk",
        kind: MetricKind::Rate,
        primary: "diskRead",
        secondary: Some("diskWrite"),
    },
    WatchMetric {
        id: "io",
        kind: MetricKind::Rate,
        primary: "ioRead",
        secondary: Some("ioWrite"),
    },
    WatchMetric {
        id: "net",
        kind: MetricKind::NetRate,
        primary: "netReceived",
        secondary: Some("netSent"),
    },
    WatchMetric {
        id: "threads",
        kind: MetricKind::Count,
        primary: "threads",
        secondary: None,
    },
    WatchMetric {
        id: "handles",
        kind: MetricKind::Count,
        primary: "handles",
        secondary: None,
    },
];

pub(super) fn metric_by_id(id: &str) -> Option<&'static WatchMetric> {
    METRICS.iter().find(|metric| metric.id == id)
}

impl WatchMetric {
    pub(super) fn is_pair(self) -> bool {
        self.secondary.is_some()
    }

    pub(super) fn short_name(self) -> String {
        match self.id {
            "memory" => label("history.watch.metric.memory"),
            "commit" => label("history.watch.metric.commit"),
            "workingSet" => label("details.workingSet"),
            "cpu" => label("history.column.moment.cpu"),
            "gpu" => label("history.column.moment.gpu"),
            "gpuMemory" => label("history.column.moment.gpuMem"),
            "disk" => label("history.column.moment.disk"),
            "io" => label("history.column.moment.io"),
            "net" => label("history.column.moment.net"),
            "threads" => label("history.column.moment.threads"),
            "handles" => label("history.column.moment.handles"),
            _ => self.id.to_owned(),
        }
    }

    pub(super) fn long_name(self) -> String {
        match self.id {
            "cpu" => label("history.reading.long.cpu"),
            "gpu" => label("history.reading.long.gpu"),
            "disk" => label("history.reading.long.disk"),
            "net" => label("history.reading.long.network"),
            "threads" => label("history.reading.long.threads"),
            "handles" => label("history.reading.long.handles"),
            _ => self.short_name(),
        }
    }

    pub(super) fn part_names(self) -> Option<(String, String)> {
        match self.id {
            "disk" | "io" => Some((label("history.part.read"), label("history.part.write"))),
            "net" => Some((label("history.part.receive"), label("history.part.send"))),
            _ => None,
        }
    }

    pub(super) fn color(self, p: Palette) -> Hsla {
        match self.id {
            "memory" => p.memory,
            "commit" => p.cpu,
            "workingSet" => p.cpu_e,
            "cpu" => p.cpu,
            "gpu" => p.gpu,
            "gpuMemory" => p.npu,
            "disk" => p.io,
            "io" => p.battery,
            "net" => p.network,
            "threads" => p.accent,
            "handles" => p.temperature,
            _ => p.accent,
        }
    }

    pub(super) fn shares_axis(self, other: Self) -> bool {
        self.kind == other.kind
    }

    pub(super) fn chart_format(self, bits: bool) -> charts::MetricFormat {
        match self.kind {
            MetricKind::Bytes => charts::MetricFormat::Memory,
            MetricKind::Percent => charts::MetricFormat::Percent,
            MetricKind::Rate => charts::MetricFormat::Rate,
            MetricKind::NetRate => charts::MetricFormat::Network(bits),
            MetricKind::Count => charts::MetricFormat::Count,
        }
    }

    pub(super) fn read_pair(self, point: &Value) -> (Option<f64>, Option<f64>) {
        let a = point[self.primary].as_f64().filter(|v| v.is_finite());
        let b = self
            .secondary
            .and_then(|field| point[field].as_f64().filter(|v| v.is_finite()));
        (a, b)
    }

    /// Headline value at a point: the primary figure, or the combined total for pair metrics.
    pub(super) fn headline_value(self, point: &Value) -> Option<f64> {
        let (a, b) = self.read_pair(point);
        if self.is_pair() {
            match (a, b) {
                (None, None) => None,
                (a, b) => Some(a.unwrap_or(0.) + b.unwrap_or(0.)),
            }
        } else {
            a
        }
    }

    /// Highest series value at a point for axis scaling.
    pub(super) fn series_max_at(self, point: &Value) -> Option<f64> {
        let (a, b) = self.read_pair(point);
        match (a, b) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (Some(a), None) | (None, Some(a)) => Some(a),
            (None, None) => None,
        }
    }

    pub(super) fn format(self, value: f64, bits: bool) -> String {
        if !value.is_finite() {
            return label("format.missing");
        }
        match self.kind {
            MetricKind::Bytes => theme::memory(value),
            MetricKind::Percent => format!("{}%", theme::cpu(value)),
            MetricKind::Rate => theme::bytes(value, true),
            MetricKind::NetRate => theme::network(value, bits),
            MetricKind::Count => {
                crate::regional::digits(&format!("{}", value.round().max(0.) as u64))
            }
        }
    }

    pub(super) fn axis_label(self, value: f64, top: f64, bits: bool) -> String {
        if value <= 0. {
            return match self.kind {
                MetricKind::Percent => "0%".into(),
                _ => "0".into(),
            };
        }
        match self.kind {
            MetricKind::Percent => {
                if top < 10. {
                    crate::regional::number!("{value:.1}%")
                } else {
                    format!("{}%", value.round() as i64)
                }
            }
            MetricKind::Bytes => {
                const KB: f64 = 1024.;
                const MB: f64 = KB * 1024.;
                const GB: f64 = MB * 1024.;
                if top >= GB {
                    compact_unit(value / GB, "GB")
                } else if top >= MB {
                    compact_unit(value / MB, "MB")
                } else if top >= KB {
                    compact_unit(value / KB, "KB")
                } else {
                    format!("{} B", value.round() as u64)
                }
            }
            MetricKind::Rate => compact_rate(value, false),
            MetricKind::NetRate => compact_rate(value, bits),
            MetricKind::Count => {
                if top >= 10_000. {
                    compact_unit(value / 1000., "k")
                } else {
                    format!("{}", value.round() as u64)
                }
            }
        }
    }

    pub(super) fn axis_top(self, max_value: f64, bits: bool) -> f64 {
        let max_value = max_value.max(0.);
        match self.kind {
            MetricKind::Percent => {
                if max_value <= 0. {
                    100.
                } else if max_value <= 10. {
                    nice_step_top(max_value, 2.)
                } else if max_value <= 25. {
                    25.
                } else if max_value <= 50. {
                    50.
                } else {
                    100.
                }
            }
            MetricKind::Bytes => {
                const MB: f64 = 1024. * 1024.;
                let floor = MB;
                nice_binary_top(max_value.max(floor))
            }
            MetricKind::Rate => {
                let floor = 1024. * 64.;
                nice_binary_top(max_value.max(floor))
            }
            MetricKind::NetRate => {
                if bits {
                    let floor = 12_500.; // 100 Kbps in bytes/s
                    nice_decimal_top((max_value * 8.).max(floor * 8.)) / 8.
                } else {
                    let floor = 1024. * 16.;
                    nice_binary_top(max_value.max(floor))
                }
            }
            MetricKind::Count => nice_step_top(max_value.max(4.), 4.),
        }
    }

    /// Representative widest values to keep reading tab widths steady.
    pub(super) fn widest_samples(self, bits: bool) -> Vec<String> {
        match self.kind {
            MetricKind::Bytes => vec!["999.9 MB".into(), "99.9 GB".into()],
            MetricKind::Percent => vec!["100.0%".into()],
            MetricKind::Rate => vec!["999.9 MB/s".into()],
            MetricKind::NetRate if bits => vec!["999.9 Mbps".into()],
            MetricKind::NetRate => vec!["999.9 MB/s".into()],
            MetricKind::Count => vec!["99,999".into()],
        }
    }
}

fn compact_unit(v: f64, unit: &str) -> String {
    if (v - v.round()).abs() < 0.05 {
        format!("{} {unit}", v.round() as i64)
    } else {
        crate::regional::number!("{v:.1} {unit}")
    }
}

fn compact_rate(bytes_per_sec: f64, bits: bool) -> String {
    if bits {
        let bps = bytes_per_sec * 8.;
        if bps >= 1e9 {
            compact_unit(bps / 1e9, "Gbps")
        } else if bps >= 1e6 {
            compact_unit(bps / 1e6, "Mbps")
        } else if bps >= 1e3 {
            compact_unit(bps / 1e3, "Kbps")
        } else {
            format!("{} bps", bps.round() as u64)
        }
    } else {
        const KB: f64 = 1024.;
        const MB: f64 = KB * 1024.;
        const GB: f64 = MB * 1024.;
        if bytes_per_sec >= GB {
            compact_unit(bytes_per_sec / GB, "GB/s")
        } else if bytes_per_sec >= MB {
            compact_unit(bytes_per_sec / MB, "MB/s")
        } else if bytes_per_sec >= KB {
            compact_unit(bytes_per_sec / KB, "KB/s")
        } else {
            format!("{} B/s", bytes_per_sec.round() as u64)
        }
    }
}

fn nice_step_top(value: f64, floor: f64) -> f64 {
    let target = (value * 1.15).max(floor);
    let pow = 10_f64.powf(target.log10().floor());
    let norm = target / pow;
    let nice = if norm <= 1. {
        1.
    } else if norm <= 2. {
        2.
    } else if norm <= 2.5 {
        2.5
    } else if norm <= 4. {
        4.
    } else if norm <= 5. {
        5.
    } else if norm <= 8. {
        8.
    } else {
        10.
    };
    (nice * pow).max(floor)
}

fn nice_decimal_top(value: f64) -> f64 {
    nice_step_top(value, 1000.)
}

fn nice_binary_top(value: f64) -> f64 {
    const KB: f64 = 1024.;
    const MB: f64 = KB * 1024.;
    const GB: f64 = MB * 1024.;
    let (unit, scaled) = if value >= GB {
        (GB, value / GB)
    } else if value >= MB {
        (MB, value / MB)
    } else if value >= KB {
        (KB, value / KB)
    } else {
        (1., value)
    };
    let target = scaled * 1.15;
    let pow = 10_f64.powf(target.max(1.).log10().floor());
    let norm = target / pow;
    let nice = if norm <= 1. {
        1.
    } else if norm <= 2. {
        2.
    } else if norm <= 4. {
        4.
    } else if norm <= 5. {
        5.
    } else if norm <= 8. {
        8.
    } else {
        10.
    };
    nice * pow * unit
}
