//! The disk details page: health, device information and protocol-specific SMART
//! log as cards, read once each time the page opens (`hardware::disk_details`).
use super::*;
mod legacy;

/// Protocol identity describes the manufacturer's device. Windows' generic
/// serial can be a namespace identifier (NGUID/EUI64), so it is the fallback.
fn identity_text<'a>(details: &'a Value, field: &str) -> Option<&'a str> {
    ["nvmeIdentity", "ataIdentity", "scsiIdentity", "identity"]
        .into_iter()
        .filter_map(|source| details[source][field].as_str())
        .map(str::trim)
        .find(|value| !value.is_empty())
}

/// Tiles under the health card's title: three a row where each gets this
/// much, else one a row (two would leave the third alone).
const TILE_MIN: f32 = 170.;
const TILE_GAP: f32 = 24.;
/// SMART attribute groups: at least this wide (at the 13 px base), up to three a row.
const GROUP_MIN: f32 = 380.;
const GROUP_GAP: f32 = 28.;
/// Remaining life at or below which the drive counts as worn (the
/// manufacturer's endurance estimate, not a failure).
const WORN_PERCENT: u64 = 10;
/// How long a first read may take before the page says it is reading.
const QUIET_WAIT: Duration = Duration::from_millis(400);

/// How the drive is doing, in order of concern.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Condition {
    Good,
    Caution,
    Bad,
}
impl Condition {
    fn key(self) -> &'static str {
        match self {
            Self::Good => "good",
            Self::Caution => "caution",
            Self::Bad => "bad",
        }
    }
}
/// Critical warning bits of the SMART/health log, the reason each names
/// (`performance.disk.reason`) and what it says about the drive: running hot
/// is a caution, the rest mean it is failing.
const WARNINGS: [(u64, &str, Condition); 6] = [
    (0x01, "spare", Condition::Bad),
    (0x02, "temperature", Condition::Caution),
    (0x04, "reliability", Condition::Bad),
    (0x08, "readOnly", Condition::Bad),
    (0x10, "backup", Condition::Bad),
    (0x20, "persistentMemory", Condition::Bad),
];
/// Bits the NVMe revisions so far leave reserved.
const RESERVED_WARNINGS: u64 = !0x3f;

/// Where a disk's details are kept in `data`, by PhysicalDrive number.
fn key(index: u64) -> String {
    format!("diskDetails.{index}")
}
/// A 128-bit counter, sent as a decimal string.
fn counter(value: &Value) -> Option<f64> {
    value.as_str().and_then(|digits| digits.parse().ok())
}
/// A counter in groups of three digits, exact at any length.
fn grouped(value: &Value) -> String {
    let digits = match value {
        Value::String(digits)
            if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) =>
        {
            digits.clone()
        }
        Value::Number(number) if number.is_u64() => number.to_string(),
        _ => return missing(),
    };
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, digit) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}
fn with_unit(value: String, unit: &str) -> String {
    if value == missing() {
        value
    } else {
        format!(
            "{value} {}",
            label(&format!("performance.disk.unit.{unit}"))
        )
    }
}
fn percent(value: &Value) -> String {
    value
        .as_u64()
        .map(|v| format!("{v}%"))
        .unwrap_or_else(missing)
}
fn sata_link(value: &Value) -> Option<String> {
    value.as_f64().map(|speed| format!("SATA {speed} Gb/s"))
}
fn ata_standard(value: &Value) -> Option<String> {
    match value.as_u64()? {
        version @ 1..=7 => Some(format!("ATA/ATAPI-{version}")),
        8 => Some("ATA8-ACS".into()),
        version @ 9..=12 => Some(format!("ACS-{}", version - 7)),
        _ => None,
    }
}
fn celsius(value: &Value) -> String {
    value
        .as_f64()
        .map(|v| format!("{} °C", v.round()))
        .unwrap_or_else(missing)
}
fn sector(bytes: u64) -> String {
    if bytes >= 1024 && bytes.is_multiple_of(1024) {
        format!("{} KB", bytes / 1024)
    } else {
        format!("{bytes} B")
    }
}
/// "PCIe 4.0 ×4" from a link's generation and width.
fn link(rate: &Value) -> Option<String> {
    let generation = rate["generation"].as_u64().filter(|g| *g > 0)?;
    let lanes = rate["lanes"].as_u64().filter(|l| *l > 0)?;
    Some(format!("PCIe {generation}.0 ×{lanes}"))
}
fn interface(bus: &str) -> String {
    match bus {
        "nvme" => "NVMe".into(),
        "sata" => "SATA".into(),
        "ata" => "ATA".into(),
        "usb" => "USB".into(),
        "scsi" => "SCSI".into(),
        "sas" => "SAS".into(),
        "raid" => "RAID".into(),
        "sd" => "SD".into(),
        "mmc" => "MMC".into(),
        "virtual" | "spaces" => label(&format!("performance.disk.bus.{bus}")),
        "" => missing(),
        _ => label("performance.disk.bus.other"),
    }
}
/// The local time of day of a Unix millisecond timestamp.
fn time_of_day(ms: u64) -> String {
    let offset = time::UtcOffset::current_local_offset().unwrap_or(time::UtcOffset::UTC);
    time::OffsetDateTime::from_unix_timestamp((ms / 1000) as i64)
        .map(|t| {
            let t = t.to_offset(offset);
            format!("{:02}:{:02}:{:02}", t.hour(), t.minute(), t.second())
        })
        .unwrap_or_else(|_| missing())
}
/// The drive's condition and the reasons for it, as keys under
/// `performance.disk.reason`: the controller's critical warnings, spare
/// below its threshold, unrecovered media errors and a worn-out endurance
/// estimate. The warning bits are kept apart from the remaining life.
fn assess(health: &Value) -> (Condition, Vec<&'static str>) {
    let bits = health["criticalWarning"].as_u64().unwrap_or(0);
    let mut condition = Condition::Good;
    let mut reasons = Vec::new();
    let mut note = |reason, level| {
        condition = condition.max(level);
        reasons.push(reason);
    };
    for (bit, reason, level) in WARNINGS {
        if bits & bit != 0 {
            note(reason, level);
        }
    }
    // A later revision may define these: worth a look, not a verdict.
    if bits & RESERVED_WARNINGS != 0 {
        note("other", Condition::Caution);
    }
    let below = health["availableSparePercent"]
        .as_u64()
        .zip(health["spareThresholdPercent"].as_u64())
        .is_some_and(|(spare, threshold)| spare < threshold);
    if below && bits & 0x01 == 0 {
        note("spare", Condition::Bad);
    }
    if counter(&health["mediaErrors"]).is_some_and(|errors| errors > 0.) {
        note("mediaErrors", Condition::Caution);
    }
    if health["remainingLifePercent"]
        .as_u64()
        .is_some_and(|life| life <= WORN_PERCENT)
    {
        note("worn", Condition::Caution);
    }
    (condition, reasons)
}
/// Splits `weights` into at most `columns` runs, in order, keeping the
/// tallest run as short as possible. Returns its height and the run lengths.
fn balance(weights: &[usize], columns: usize) -> (usize, Vec<usize>) {
    if columns <= 1 || weights.len() <= 1 {
        return (weights.iter().sum(), vec![weights.len()]);
    }
    (1..weights.len())
        .map(|first| {
            let (rest, mut runs) = balance(&weights[first..], columns - 1);
            runs.insert(0, first);
            (rest.max(weights[..first].iter().sum()), runs)
        })
        .min_by_key(|(tallest, _)| *tallest)
        .expect("at least two weights")
}
fn reason_list(reasons: &[&str]) -> String {
    reasons
        .iter()
        .map(|reason| label(&format!("performance.disk.reason.{reason}")))
        .collect::<Vec<_>>()
        .join(&label("common.listSeparator"))
}

/// One row of the SMART table: the reading and the field as the drive stores it.
struct Attribute {
    name: String,
    value: String,
    raw: String,
    alert: bool,
}
fn attribute(key: &str, value: String, raw: String) -> Attribute {
    Attribute {
        name: label(&format!("performance.disk.attribute.{key}")),
        value,
        raw,
        alert: false,
    }
}
/// The SMART/health log's fields in three groups: health, usage and temperature.
fn attributes(health: &Value) -> Vec<(&'static str, Vec<Attribute>)> {
    let bits = health["criticalWarning"].as_u64().unwrap_or(0);
    let raw = |key: &str| grouped(&health[key]);
    let warnings = WARNINGS
        .iter()
        .filter(|(bit, ..)| bits & bit != 0)
        .map(|(_, reason, _)| *reason)
        .chain((bits & RESERVED_WARNINGS != 0).then_some("other"))
        .collect::<Vec<_>>();
    let spare_low = health["availableSparePercent"]
        .as_u64()
        .zip(health["spareThresholdPercent"].as_u64())
        .is_some_and(|(spare, threshold)| spare < threshold);
    let state = vec![
        Attribute {
            alert: bits != 0,
            ..attribute(
                "criticalWarning",
                if warnings.is_empty() {
                    label("performance.disk.noWarnings")
                } else {
                    reason_list(&warnings)
                },
                format!("0x{bits:02X}"),
            )
        },
        Attribute {
            alert: spare_low,
            ..attribute(
                "availableSpare",
                percent(&health["availableSparePercent"]),
                raw("availableSparePercent"),
            )
        },
        attribute(
            "spareThreshold",
            percent(&health["spareThresholdPercent"]),
            raw("spareThresholdPercent"),
        ),
        Attribute {
            alert: health["remainingLifePercent"]
                .as_u64()
                .is_some_and(|life| life <= WORN_PERCENT),
            ..attribute(
                "percentageUsed",
                percent(&health["percentageUsed"]),
                raw("percentageUsed"),
            )
        },
        Attribute {
            alert: counter(&health["mediaErrors"]).is_some_and(|errors| errors > 0.),
            ..attribute("mediaErrors", raw("mediaErrors"), raw("mediaErrors"))
        },
        attribute(
            "errorLogEntries",
            raw("errorLogEntries"),
            raw("errorLogEntries"),
        ),
    ];
    let bytes = |key: &str| {
        counter(&health[key])
            .map(fmt_capacity)
            .unwrap_or_else(missing)
    };
    let usage = vec![
        attribute(
            "dataUnitsRead",
            bytes("hostReadBytes"),
            raw("dataUnitsRead"),
        ),
        attribute(
            "dataUnitsWritten",
            bytes("hostWrittenBytes"),
            raw("dataUnitsWritten"),
        ),
        attribute(
            "hostReadCommands",
            raw("hostReadCommands"),
            raw("hostReadCommands"),
        ),
        attribute(
            "hostWriteCommands",
            raw("hostWriteCommands"),
            raw("hostWriteCommands"),
        ),
        attribute(
            "controllerBusyTime",
            with_unit(raw("controllerBusyMinutes"), "minutes"),
            raw("controllerBusyMinutes"),
        ),
        attribute("powerCycles", raw("powerCycles"), raw("powerCycles")),
        attribute(
            "powerOnHours",
            with_unit(raw("powerOnHours"), "hours"),
            raw("powerOnHours"),
        ),
        attribute(
            "unsafeShutdowns",
            raw("unsafeShutdowns"),
            raw("unsafeShutdowns"),
        ),
    ];
    let mut temperature = vec![attribute(
        "compositeTemperature",
        celsius(&health["temperature"]["celsius"]),
        grouped(&health["temperature"]["kelvin"]),
    )];
    // Slots the drive leaves unreported are left out; their numbers stay.
    for (slot, sensor) in values(&health["temperatureSensors"]).iter().enumerate() {
        if sensor["celsius"].is_number() {
            temperature.push(Attribute {
                name: tr!("performance.disk.attribute.sensor", index = slot + 1),
                value: celsius(&sensor["celsius"]),
                raw: grouped(&sensor["kelvin"]),
                alert: false,
            });
        }
    }
    temperature.push(attribute(
        "warningTemperatureTime",
        with_unit(raw("warningTemperatureMinutes"), "minutes"),
        raw("warningTemperatureMinutes"),
    ));
    temperature.push(attribute(
        "criticalTemperatureTime",
        with_unit(raw("criticalTemperatureMinutes"), "minutes"),
        raw("criticalTemperatureMinutes"),
    ));
    let thermal = &health["thermalManagement"];
    if thermal.is_object() {
        for level in [1, 2] {
            let field = format!("threshold{level}Transitions");
            temperature.push(Attribute {
                name: tr!(
                    "performance.disk.attribute.thermalTransitions",
                    index = level
                ),
                value: grouped(&thermal[&field]),
                raw: grouped(&thermal[&field]),
                alert: false,
            });
        }
        for level in [1, 2] {
            let field = format!("threshold{level}Seconds");
            temperature.push(Attribute {
                name: tr!("performance.disk.attribute.thermalTime", index = level),
                value: with_unit(grouped(&thermal[&field]), "seconds"),
                raw: grouped(&thermal[&field]),
                alert: false,
            });
        }
    }
    vec![
        ("health", state),
        ("usage", usage),
        ("temperature", temperature),
    ]
}

impl Desktop {
    /// Reads a disk's details once, on a worker; the answer replaces the last.
    pub(in crate::frontend) fn read_disk_details(
        &mut self,
        index: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.request(
            "disk_details",
            json!({ "index": index }),
            Some(&key(index)),
            window,
            cx,
        );
    }
    /// Reads the shown disk's details each time its details page opens:
    /// another disk, the overview and back, or the performance page again.
    pub(super) fn open_disk_details(
        &mut self,
        device: Option<&Device>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let index = device
            .filter(|device| device.kind == "disk" && self.disk_page == "details")
            .and_then(|device| {
                self.snapshot["hardware"]["disks"][device.index]["info"]["index"].as_u64()
            });
        let open = index.map(|index| (index, self.page_epoch));
        if open
            == self
                .disk_details_open
                .map(|(index, epoch, _)| (index, epoch))
        {
            return;
        }
        self.disk_details_open = open.map(|(index, epoch)| (index, epoch, Instant::now()));
        if let Some(index) = index {
            self.read_disk_details(index, window, cx);
        }
    }
    /// The head's button reading the details again.
    pub(super) fn disk_refresh_button(&self, index: u64, cx: &mut Context<Self>) -> AnyElement {
        self.btn_small(
            "disk-details-refresh",
            "",
            Some("RefreshIcon"),
            widgets::Tone::Ghost,
        )
        .accessibility_label(label("performance.disk.refresh"))
        .tooltip(|window, cx| widgets::tooltip(label("performance.disk.refresh"), window, cx))
        .on_click(cx.listener(move |this, _, window, cx| {
            this.act(
                Action::Preference("refreshDiskDetails".into(), json!(index)),
                window,
                cx,
            )
        }))
        .into_any_element()
    }
    pub(super) fn disk_details(
        &self,
        device: &Device,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let disk = &self.snapshot["hardware"]["disks"][device.index];
        let Some(index) = disk["info"]["index"].as_u64() else {
            return div().into_any_element();
        };
        let Some(details) = self.data.get(&key(index)) else {
            return self.disk_details_waiting(index);
        };
        let mut read_at = details["queriedAt"]
            .as_u64()
            .map(|ms| tr!("performance.disk.readAt", time = time_of_day(ms)));
        let failure = |query: &str| {
            values(&details["queries"])
                .iter()
                .find(|status| text(status, "query") == query)
                .filter(|status| text(status, "state") != "ready")
                .cloned()
        };
        if let Some(status) = failure("open") {
            return self.card(
                label("performance.disk.device"),
                None,
                read_at,
                false,
                self.disk_note(label("performance.disk.openFailed"), &status),
            );
        }
        // The card's content box: its padding and border.
        let width = (self.detail_width(window) - 34.).max(160.);
        let health = &details["nvmeHealth"];
        let mut cards = Vec::new();
        if health.is_object() {
            cards.push(self.disk_health(disk, details, read_at.take(), width, cx));
        } else if details["smartHealth"].is_object() {
            cards.push(self.disk_legacy_health(disk, details, read_at.take(), width));
        } else {
            let nvme = text(&details["identity"], "bus") == "nvme";
            let status = if nvme {
                failure("nvmeHealth")
            } else {
                failure("service")
                    .or_else(|| failure("smartAccess"))
                    .or_else(|| failure("ataAttributes"))
                    .or_else(|| failure("scsiHealth"))
                    .or_else(|| failure("failurePrediction"))
            };
            let message = label(&format!(
                "performance.disk.healthMissing.{}",
                match &status {
                    _ if details["ataIdentity"]["smartEnabled"].as_bool() == Some(false) =>
                        "smartDisabled",
                    Some(status) => text(status, "state"),
                    None => "unsupported",
                }
            ));
            cards.push(self.card(
                label("performance.disk.health"),
                None,
                read_at.take(),
                false,
                self.disk_note(message, status.as_ref().unwrap_or(&Value::Null)),
            ));
        }
        cards.push(self.disk_device(details, index, width, window, cx));
        if health.is_object() {
            cards.push(self.card(
                label("performance.disk.smart"),
                Some(label("performance.disk.smartSource")),
                None,
                false,
                self.disk_attributes(health, width),
            ));
        } else if details["smartHealth"].is_object() {
            cards.extend(self.disk_legacy_attributes(details));
        }
        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .children(cards)
            .into_any_element()
    }
    /// Before the first answer: nothing for a moment (a read takes a few
    /// milliseconds), then a note that it is reading.
    fn disk_details_waiting(&self, index: u64) -> AnyElement {
        let p = self.palette;
        let key = key(index);
        let reading = self
            .pending
            .values()
            .any(|pending| pending.cache.as_deref() == Some(key.as_str()));
        let quiet = self
            .disk_details_open
            .is_some_and(|(_, _, at)| at.elapsed() < QUIET_WAIT);
        if reading && quiet {
            return div().into_any_element();
        }
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .text_size(theme::font(12.))
            .text_color(p.muted)
            .when(reading, |d| d.child(self.spinner(12., p.accent)))
            .child(label(if reading {
                "performance.disk.loading"
            } else {
                "performance.disk.unavailable"
            }))
            .into_any_element()
    }
    /// Why a part of the details is missing, with the Windows error code if any.
    fn disk_note(&self, message: String, status: &Value) -> AnyElement {
        let p = self.palette;
        div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .text_color(p.muted)
            .child(message)
            .when_some(status["win32Error"].as_u64(), |d, code| {
                d.child(
                    div()
                        .text_size(theme::font(12.))
                        .text_color(p.faint)
                        .child(tr!("performance.disk.errorCode", code = code)),
                )
            })
            .into_any_element()
    }
    /// A level bar under a tile's figure: `level` of the track (0–1), with
    /// an optional mark at `mark`, such as a threshold.
    fn disk_meter(&self, level: f64, mark: Option<f64>, color: Hsla) -> Div {
        let unit = |value: f64| {
            if value.is_finite() {
                value.clamp(0., 1.) as f32
            } else {
                0.
            }
        };
        div()
            .relative()
            .mt(px(10.))
            .h(px(6.))
            .rounded(px(3.))
            .bg(color.opacity(0.18))
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left_0()
                    .w(relative(unit(level)))
                    .rounded(px(3.))
                    .bg(color),
            )
            .when_some(mark, |d, mark| {
                d.child(
                    div()
                        .absolute()
                        .top(px(-3.))
                        .h(px(12.))
                        .left(relative(unit(mark)))
                        .ml(px(-1.))
                        .w(px(2.))
                        .rounded(px(1.))
                        .bg(self.palette.ink.opacity(0.5)),
                )
            })
    }
    /// A figure, its bar and a line or two under it.
    fn disk_tile(
        &self,
        figure: AnyElement,
        meter: Div,
        caption: Option<String>,
        alert: Option<(String, Hsla)>,
    ) -> AnyElement {
        let p = self.palette;
        let line = |text: String, color: Hsla| {
            div()
                .mt(px(6.))
                .text_size(theme::font(12.))
                .text_color(color)
                .child(text)
        };
        div()
            .min_w_0()
            .flex()
            .flex_col()
            .child(figure)
            .child(meter)
            .when_some(caption, |d, caption| d.child(line(caption, p.muted)))
            .when_some(alert, |d, (text, color)| d.child(line(text, color)))
            .into_any_element()
    }
    /// The drive's condition, remaining life, temperature and spare as bars,
    /// with its lifetime totals under them.
    fn disk_health(
        &self,
        disk: &Value,
        details: &Value,
        read_at: Option<String>,
        width: f32,
        cx: &App,
    ) -> AnyElement {
        let p = self.palette;
        let health = &details["nvmeHealth"];
        let identity = &details["nvmeIdentity"];
        let (condition, reasons) = assess(health);
        let tone = match condition {
            Condition::Good => p.network,
            Condition::Caution => p.io,
            Condition::Bad => p.danger,
        };
        let life = health["remainingLifePercent"].as_u64().unwrap_or(0);
        let state = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(div().size(px(10.)).flex_shrink_0().rounded_full().bg(tone))
            .child(label(&format!(
                "performance.disk.status.{}",
                condition.key()
            )))
            .into_any_element();
        let mut tiles = vec![self.disk_tile(
            self.stat_element(label("common.status"), state, None, cx),
            self.disk_meter(
                life as f64 / 100.,
                None,
                if life <= WORN_PERCENT { p.danger } else { p.io },
            ),
            Some(tr!("performance.disk.lifeLeft", percent = life)),
            (!reasons.is_empty()).then(|| {
                (
                    reason_list(&reasons),
                    if condition == Condition::Bad {
                        p.danger
                    } else {
                        p.ink
                    },
                )
            }),
        )];
        // The live reading, as on the overview; the log's if there is none.
        let temperature = disk["temperature"]
            .as_f64()
            .filter(|v| *v != 0.)
            .or_else(|| health["temperature"]["celsius"].as_f64());
        let warning = identity["warningTemperatureCelsius"].as_f64();
        let critical = identity["criticalTemperatureCelsius"].as_f64();
        let top = critical.or(warning).unwrap_or(100.).max(1.);
        let hot = temperature
            .zip(warning)
            .is_some_and(|(celsius, warning)| celsius >= warning);
        let limits = [
            warning.map(|v| tr!("performance.disk.warningAt", value = v.round())),
            critical.map(|v| tr!("performance.disk.criticalAt", value = v.round())),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
        tiles.push(
            self.disk_tile(
                self.stat(
                    label("common.resource.temperature"),
                    temperature
                        .map(|v| v.round().to_string())
                        .unwrap_or_else(missing),
                    Some("°C".into()),
                    None,
                    cx,
                ),
                self.disk_meter(
                    temperature.unwrap_or(0.) / top,
                    warning.filter(|_| critical.is_some()).map(|v| v / top),
                    if hot { p.danger } else { p.io },
                ),
                (!limits.is_empty()).then(|| limits.join(" · ")),
                None,
            ),
        );
        let spare = health["availableSparePercent"].as_u64();
        let threshold = health["spareThresholdPercent"].as_u64();
        let low = spare
            .zip(threshold)
            .is_some_and(|(spare, threshold)| spare < threshold);
        tiles.push(self.disk_tile(
            self.stat(
                label("performance.disk.spare"),
                spare.map(|v| v.to_string()).unwrap_or_else(missing),
                Some("%".into()),
                None,
                cx,
            ),
            self.disk_meter(
                spare.unwrap_or(0) as f64 / 100.,
                threshold.map(|v| v as f64 / 100.),
                if low { p.danger } else { p.io },
            ),
            threshold.map(|v| tr!("performance.disk.spareThreshold", percent = v)),
            None,
        ));
        let columns = if width >= 3. * TILE_MIN + 2. * TILE_GAP {
            3
        } else {
            1
        };
        let total = |key: &str, title: &str| {
            let (value, unit) = split(
                counter(&health[key])
                    .map(fmt_capacity)
                    .unwrap_or_else(missing),
            );
            self.sensor_reading(label(title), value, unit.as_deref().unwrap_or(""))
                .into_any_element()
        };
        let count = |key: &str, title: &str, unit: Option<&str>| {
            self.sensor_reading(
                label(title),
                grouped(&health[key]),
                &unit
                    .map(|unit| label(&format!("performance.disk.unit.{unit}")))
                    .unwrap_or_default(),
            )
            .into_any_element()
        };
        let readings = vec![
            total("hostReadBytes", "performance.disk.totalRead"),
            total("hostWrittenBytes", "performance.disk.totalWritten"),
            count(
                "powerOnHours",
                "performance.disk.powerOnHours",
                Some("hours"),
            ),
            count("powerCycles", "performance.disk.powerCycles", None),
            count("unsafeShutdowns", "performance.disk.unsafeShutdowns", None),
            count("mediaErrors", "performance.disk.mediaErrors", None),
        ];
        let body = div()
            .flex()
            .flex_col()
            .child(
                div()
                    .grid()
                    .grid_cols(columns)
                    .gap_x(px(TILE_GAP))
                    .gap_y(px(18.))
                    .children(tiles),
            )
            .child(
                div()
                    .mt(px(18.))
                    .pt(px(16.))
                    .border_t_1()
                    .border_color(p.stroke)
                    .child(self.reading_grid(readings, width)),
            );
        self.card(
            label("performance.disk.health"),
            None,
            read_at,
            condition == Condition::Bad,
            body,
        )
    }
    /// Identity, link, standard and capabilities, in two lists where they fit.
    fn disk_device(
        &self,
        details: &Value,
        index: u64,
        width: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.palette;
        let identity = &details["identity"];
        let nvme = &details["nvmeIdentity"];
        let ata = &details["ataIdentity"];
        let plain = |value: String| div().child(value).into_any_element();
        let either = |key: &str| {
            identity_text(details, key)
                .map(String::from)
                .unwrap_or_else(missing)
        };
        let capacity = details["capacityBytes"]
            .as_u64()
            .filter(|bytes| *bytes > 0)
            .map(|bytes| {
                let exact = tr!(
                    "performance.disk.bytes",
                    count = grouped(&details["capacityBytes"])
                );
                tr!(
                    "performance.withAlt",
                    value = fmt_capacity(bytes as f64),
                    alt = exact
                )
            })
            .unwrap_or_else(missing);
        let volumes = values(&details["volumes"])
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(" ");
        let identity_rows = vec![
            (label("performance.disk.model"), plain(either("model"))),
            (
                label("performance.disk.firmware"),
                plain(either("firmwareRevision")),
            ),
            (
                label("performance.disk.serial"),
                self.disk_serial(identity_text(details, "serialNumber"), index, window, cx),
            ),
            (label("performance.disk.capacity"), plain(capacity)),
            (
                label("performance.disk.volumes"),
                plain(if volumes.is_empty() {
                    label("common.none")
                } else {
                    volumes
                }),
            ),
        ];
        let mut link_rows = vec![(
            label("performance.disk.interface"),
            plain(interface(text(identity, "bus"))),
        )];
        let pcie = &details["pcieLink"];
        let current = link(&pcie["current"]).or_else(|| sata_link(&ata["sataCurrentGbps"]));
        let maximum = link(&pcie["maximum"]).or_else(|| sata_link(&ata["sataMaximumGbps"]));
        if let Some(transfer) = match (current, maximum) {
            (Some(current), Some(maximum)) if current != maximum => Some(tr!(
                "performance.disk.linkMaximum",
                current = current,
                maximum = maximum
            )),
            (Some(current), _) => Some(current),
            (None, maximum) => maximum,
        } {
            link_rows.push((label("performance.disk.transfer"), plain(transfer)));
        }
        if let Some(version) = nvme["version"].as_object() {
            let part = |key: &str| version.get(key).and_then(Value::as_u64).unwrap_or(0);
            let tertiary = part("tertiary");
            link_rows.push((
                label("performance.disk.standard"),
                plain(if tertiary > 0 {
                    format!("NVMe {}.{}.{tertiary}", part("major"), part("minor"))
                } else {
                    format!("NVMe {}.{}", part("major"), part("minor"))
                }),
            ));
        } else if let Some(standard) = ata_standard(&ata["majorVersion"]) {
            link_rows.push((label("performance.disk.standard"), plain(standard)));
        }
        if let (Some(logical), Some(physical)) = (
            details["logicalSectorBytes"].as_u64(),
            details["physicalSectorBytes"].as_u64(),
        ) {
            link_rows.push((
                label("performance.disk.sectors"),
                plain(tr!(
                    "performance.disk.sectorSizes",
                    logical = sector(logical),
                    physical = sector(physical)
                )),
            ));
        }
        let features = [
            (
                details["nvmeHealth"].is_object()
                    || details["smartHealth"].is_object()
                    || boolean(&details["ataIdentity"], "smartSupported"),
                "smart",
            ),
            (
                boolean(details, "trimEnabled") || boolean(nvme, "datasetManagement"),
                "trim",
            ),
            (
                boolean(nvme, "volatileWriteCache") || boolean(ata, "writeCacheSupported"),
                "writeCache",
            ),
            (boolean(ata, "ncqSupported"), "ncq"),
        ]
        .into_iter()
        .filter(|(present, _)| *present)
        .map(|(_, feature)| label(&format!("performance.disk.feature.{feature}")))
        .collect::<Vec<_>>();
        if !features.is_empty() {
            // Capabilities as tags, as the CPU's instruction sets.
            link_rows.push((
                label("performance.disk.features"),
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(4.))
                    .children(features.into_iter().map(|name| {
                        div()
                            .px(px(6.))
                            .py(px(1.))
                            .rounded(px(4.))
                            .bg(p.ink.opacity(0.06))
                            .text_color(p.muted)
                            .text_size(theme::font(11.))
                            .line_height(px(16.))
                            .child(name)
                    }))
                    .into_any_element(),
            ));
        }
        if let Some(rpm) = details["ataIdentity"]["rotationRpm"].as_u64() {
            link_rows.push((
                label("performance.disk.rotation"),
                plain(tr!("performance.disk.rpm", value = rpm)),
            ));
        }
        let wide = width >= 2. * 300. + GROUP_GAP;
        let body = if wide {
            div()
                .grid()
                .grid_cols(2)
                .gap_x(px(GROUP_GAP))
                // A shorter list keeps its row spacing.
                .items_start()
                .child(self.info_list(identity_rows))
                .child(self.info_list(link_rows))
                .into_any_element()
        } else {
            self.info_list(identity_rows.into_iter().chain(link_rows).collect())
        };
        self.card(label("performance.disk.device"), None, None, false, body)
    }
    /// The serial number, hidden until asked for: pages like this one end up
    /// in screenshots.
    fn disk_serial(
        &self,
        serial: Option<&str>,
        index: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(serial) = serial else {
            return div().child(missing()).into_any_element();
        };
        let p = self.palette;
        let state = window.use_keyed_state(
            SharedString::from(format!("disk-serial-{index}")),
            cx,
            |_, _| false,
        );
        let shown = *state.read(cx);
        let owner = cx.entity().downgrade();
        div()
            .flex()
            .items_baseline()
            .gap(px(10.))
            .child(if shown {
                serial.to_owned()
            } else {
                "•".repeat(12)
            })
            .child(
                div()
                    .id(SharedString::from(format!("disk-serial-toggle-{index}")))
                    .text_size(theme::font(12.))
                    .text_color(p.accent)
                    .cursor_pointer()
                    .child(label(if shown {
                        "performance.disk.hide"
                    } else {
                        "performance.disk.show"
                    }))
                    .on_click(move |_, _, cx| {
                        state.update(cx, |shown, _| *shown = !*shown);
                        let _ = owner.update(cx, |_, cx| cx.notify());
                    }),
            )
            .into_any_element()
    }
    /// The SMART/health log as tables, one per group, in columns of even
    /// height where they fit.
    fn disk_attributes(&self, health: &Value, width: f32) -> AnyElement {
        let p = self.palette;
        let group_min = f32::from(theme::font(GROUP_MIN));
        let columns = ((width + GROUP_GAP) / (group_min + GROUP_GAP))
            .floor()
            .clamp(1., 3.) as usize;
        let row = |name: AnyElement, value: AnyElement, raw: AnyElement| {
            div()
                .flex()
                .items_baseline()
                .gap(px(12.))
                .py(px(6.))
                .border_b_1()
                .border_color(p.stroke)
                .child(div().flex_1().min_w_0().child(name))
                .child(
                    div()
                        .w(theme::font(96.))
                        .flex_shrink_0()
                        .flex()
                        .justify_end()
                        .text_right()
                        .child(value),
                )
                .child(
                    div()
                        .w(theme::font(92.))
                        .flex_shrink_0()
                        .flex()
                        .justify_end()
                        .child(raw),
                )
        };
        let groups = attributes(health);
        // Each group's rows plus its header and the gap under it.
        let weights = groups
            .iter()
            .map(|(_, list)| list.len() + 2)
            .collect::<Vec<_>>();
        let (_, runs) = balance(&weights, columns);
        let mut groups = groups.into_iter().map(|(group, list)| {
            let head = row(
                div()
                    .text_size(theme::font(13.))
                    .font_weight(theme::LABEL_WEIGHT)
                    .text_color(p.ink)
                    .child(label(&format!("performance.disk.group.{group}")))
                    .into_any_element(),
                div()
                    .child(label("performance.disk.column.value"))
                    .into_any_element(),
                div()
                    .child(label("performance.disk.column.raw"))
                    .into_any_element(),
            )
            .text_color(p.muted)
            .pt(px(0.));
            div()
                .min_w_0()
                .flex()
                .flex_col()
                .text_size(theme::font(12.))
                .child(head)
                .children(list.into_iter().map(|attribute| {
                    row(
                        div().child(attribute.name).into_any_element(),
                        div()
                            .when(attribute.alert, |d| d.text_color(p.danger))
                            .child(attribute.value)
                            .into_any_element(),
                        div()
                            .text_color(p.muted)
                            .child(attribute.raw)
                            .into_any_element(),
                    )
                }))
                .into_any_element()
        });
        let columns = runs
            .into_iter()
            .map(|run| {
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(20.))
                    .children(groups.by_ref().take(run).collect::<Vec<_>>())
            })
            .collect::<Vec<_>>();
        div()
            .flex()
            .items_start()
            .gap(px(GROUP_GAP))
            .children(columns)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    // Not a glob: GPUI's own `test` attribute would shadow the standard one.
    use super::{
        Condition, Value, assess, attributes, balance, grouped, identity_text, link, sector,
    };
    use serde_json::json;

    #[test]
    fn manufacturer_serial_precedes_namespace_and_bridge_identity() {
        let mut details = json!({"identity": {"serialNumber": "0000_0000_0000_0000_0123_4567_89AB_CDEF."},
            "nvmeIdentity": {"serialNumber": "  0000FactorySN  "}, "ataIdentity": {"serialNumber": "AtaSN"}});
        assert_eq!(
            identity_text(&details, "serialNumber"),
            Some("0000FactorySN")
        );
        details["nvmeIdentity"]["serialNumber"] = json!(" ");
        assert_eq!(identity_text(&details, "serialNumber"), Some("AtaSN"));
        details["ataIdentity"] = Value::Null;
        assert_eq!(
            identity_text(&details, "serialNumber"),
            details["identity"]["serialNumber"].as_str()
        );
    }

    fn healthy() -> Value {
        json!({
            "criticalWarning": 0,
            "availableSparePercent": 100,
            "spareThresholdPercent": 1,
            "percentageUsed": 2,
            "remainingLifePercent": 98,
            "mediaErrors": "0",
        })
    }

    #[test]
    fn a_quiet_log_is_good() {
        assert_eq!(assess(&healthy()), (Condition::Good, vec![]));
    }

    #[test]
    fn warnings_are_weighed_apart_from_remaining_life() {
        let mut health = healthy();
        health["criticalWarning"] = json!(0x02);
        assert_eq!(assess(&health), (Condition::Caution, vec!["temperature"]));
        health["criticalWarning"] = json!(0x0c);
        assert_eq!(
            assess(&health),
            (Condition::Bad, vec!["reliability", "readOnly"])
        );
        health["criticalWarning"] = json!(0x40);
        assert_eq!(assess(&health), (Condition::Caution, vec!["other"]));
    }

    #[test]
    fn spare_errors_and_wear_are_read_from_the_counters() {
        let mut health = healthy();
        health["availableSparePercent"] = json!(0);
        health["spareThresholdPercent"] = json!(10);
        assert_eq!(assess(&health), (Condition::Bad, vec!["spare"]));
        // The drive's own bit names the spare once.
        health["criticalWarning"] = json!(0x01);
        assert_eq!(assess(&health), (Condition::Bad, vec!["spare"]));
        let mut health = healthy();
        health["mediaErrors"] = json!("3");
        health["remainingLifePercent"] = json!(0);
        assert_eq!(
            assess(&health),
            (Condition::Caution, vec!["mediaErrors", "worn"])
        );
    }

    #[test]
    fn counters_group_exactly_beyond_double_precision() {
        assert_eq!(grouped(&json!("0")), "0");
        assert_eq!(grouped(&json!("1195008152")), "1,195,008,152");
        assert_eq!(
            grouped(&json!(u128::MAX.to_string())),
            "340,282,366,920,938,463,463,374,607,431,768,211,455"
        );
        assert_eq!(grouped(&json!(3799)), "3,799");
    }

    #[test]
    fn groups_fill_columns_in_order_and_evenly() {
        // Health, usage and temperature of a drive with two sensors.
        assert_eq!(balance(&[8, 10, 13], 2), (18, vec![2, 1]));
        assert_eq!(balance(&[8, 10, 13], 3), (13, vec![1, 1, 1]));
        assert_eq!(balance(&[8, 10, 13], 1), (31, vec![3]));
        assert_eq!(balance(&[8, 10, 20], 2), (20, vec![2, 1]));
        assert_eq!(balance(&[20, 4, 4], 2), (20, vec![1, 2]));
    }

    #[test]
    fn links_name_the_generation_and_width() {
        assert_eq!(
            link(&json!({"generation": 4, "lanes": 4})).as_deref(),
            Some("PCIe 4.0 ×4")
        );
        assert!(link(&json!({"generation": 0, "lanes": 4})).is_none());
        assert!(link(&Value::Null).is_none());
        assert_eq!(sector(512), "512 B");
        assert_eq!(sector(4096), "4 KB");
    }

    #[test]
    fn unreported_sensor_slots_are_left_out() {
        let mut health = healthy();
        health["temperature"] = json!({"kelvin": 324, "celsius": 50.85});
        health["temperatureSensors"] = json!([
            {"kelvin": 324, "celsius": 50.85},
            {"kelvin": 0, "celsius": null},
            {"kelvin": 316, "celsius": 42.85},
        ]);
        let groups = attributes(&health);
        let (_, temperature) = &groups[2];
        let raw = temperature
            .iter()
            .map(|row| row.raw.as_str())
            .collect::<Vec<_>>();
        assert_eq!(&raw[..3], ["324", "324", "316"]);
        assert!(groups[0].1[0].raw == "0x00" && !groups[0].1[0].alert);
    }
}
