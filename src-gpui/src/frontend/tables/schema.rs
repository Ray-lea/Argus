//! The columns of each table: the kind of value each shows, the track its
//! width follows and its label.
use super::*;

#[derive(Clone, Copy, PartialEq)]
pub(super) enum CellKind {
    Dim,
    Mono,
    MonoDim,
    Count,
    Pid,
    ServicePid,
    Port,
    PortDim,
    Percent,
    Memory,
    Rate,
    /// Events per second.
    EventRate,
    Name,
    Protocol,
    ConnectionState,
    ServiceState,
    StartType,
    Enabled,
    StartupLocation,
    ProcessText,
    Identifier,
    NetworkRate,
    Duration,
    Timestamp,
    ProcessBoolean,
    AddressSpace,
    /// A power estimate shown as its level.
    Power,
}
/// A CSS grid track from the tables: a fixed width or `minmax(min, fr)`.
/// The service and startup lists size theirs by their contents instead
/// (`widths::fill`).
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Track {
    Fixed(f32),
    Flex(f32, f32),
    /// As wide as the contents, between a minimum and a maximum.
    Fit(f32, f32),
    /// The last column of a list: the room the others leave, at least this wide.
    Fill(f32),
}
/// Whether `page`'s table sizes its columns by their contents, its last
/// column taking the room the others leave (`widths::fill`).
pub(in crate::frontend) fn fills(page: &str) -> bool {
    matches!(page, "services" | "startup")
}
#[derive(Clone)]
pub(super) struct Field {
    pub(super) key: &'static str,
    pub(super) label: String,
    pub(super) kind: CellKind,
    pub(super) track: Track,
    pub(super) end: bool,
}
pub(super) fn fields(page: &str, sources: columns::Sources, _available: f32) -> Vec<Field> {
    use CellKind::*;
    use Track::*;
    // The lists' label columns are as wide as their widest label
    // (`fit_fixed`); the others follow the list (`widths::fill`).
    let definitions: Vec<(&str, &str, CellKind, Track)> = match page {
        "services" => vec![
            (
                "displayName",
                "services.column.display",
                Name,
                Fit(160., 360.),
            ),
            ("name", "common.name", Dim, Fit(96., 280.)),
            (
                "pid",
                "processes.column.pid.label",
                ServicePid,
                Fit(56., 120.),
            ),
            ("state", "common.status", ServiceState, Fixed(0.)),
            (
                "startType",
                "services.column.startType",
                StartType,
                Fixed(0.),
            ),
            (
                "description",
                "services.column.description",
                Dim,
                Fill(240.),
            ),
        ],
        "startup" => vec![
            ("name", "common.name", Name, Fit(160., 360.)),
            (
                "publisher",
                "startup.column.publisher",
                Dim,
                Fit(100., 280.),
            ),
            ("enabled", "common.status", Enabled, Fixed(0.)),
            (
                "location",
                "startup.column.location",
                StartupLocation,
                Fit(100., 280.),
            ),
            ("command", "common.command", MonoDim, Fill(240.)),
        ],
        "network" => vec![
            ("name", "connections.column.process", Name, Flex(180., 1.1)),
            ("pid", "processes.column.pid.label", Pid, Fixed(72.)),
            (
                "protocol",
                "connections.column.protocol",
                Protocol,
                // Its header also holds the protocol filter's button.
                Fixed(104.),
            ),
            (
                "localAddress",
                "connections.column.localAddress",
                Mono,
                Flex(130., 0.9),
            ),
            (
                "localPort",
                "connections.column.localPort",
                Port,
                Fixed(88.),
            ),
            (
                "remoteAddress",
                "connections.column.remoteAddress",
                MonoDim,
                Flex(130., 0.9),
            ),
            (
                "remotePort",
                "connections.column.remotePort",
                PortDim,
                Fixed(88.),
            ),
            ("state", "common.status", ConnectionState, Fixed(96.)),
        ],
        _ => return process_fields(&columns::Preferences::default(), sources),
    };
    definitions
        .into_iter()
        .map(|(key, title, kind, track)| Field {
            key,
            label: label(title),
            kind,
            track,
            end: matches!(
                kind,
                Count | Pid | ServicePid | Port | PortDim | Percent | Memory | Rate
            ),
        })
        .collect()
}

pub(super) fn process_fields(
    preferences: &columns::Preferences,
    sources: columns::Sources,
) -> Vec<Field> {
    use columns::Format;
    preferences
        .visible(sources)
        .into_iter()
        .map(|column| {
            let kind = match column.format {
                Format::Name => CellKind::Name,
                Format::Text => CellKind::ProcessText,
                Format::Identifier if column.key == "pid" => CellKind::Pid,
                Format::Identifier => CellKind::Identifier,
                Format::Count => CellKind::Count,
                Format::Percent => CellKind::Percent,
                Format::Memory => CellKind::Memory,
                Format::AddressSpace => CellKind::AddressSpace,
                Format::Rate => CellKind::Rate,
                Format::EventRate => CellKind::EventRate,
                Format::Network => CellKind::NetworkRate,
                Format::Duration => CellKind::Duration,
                Format::Timestamp => CellKind::Timestamp,
                Format::Boolean => CellKind::ProcessBoolean,
                Format::Power => CellKind::Power,
            };
            Field {
                key: column.key,
                label: label(&format!("processes.column.{}.label", column.key)),
                kind,
                // Automatic widths are settled in `NativeTable::configure`;
                // until then a column takes its designed width.
                track: Track::Fixed(if preferences.auto_width {
                    column.width
                } else {
                    preferences.set_width(column.key).unwrap_or(column.width)
                }),
                end: !matches!(
                    column.format,
                    Format::Name
                        | Format::Text
                        | Format::Timestamp
                        | Format::Boolean
                        | Format::Power
                ),
            }
        })
        .collect()
}
/// The labels a fixed column's cells can show, as locale keys under `prefix`,
/// and the width of what sits beside the label (the status dot and its gap).
pub(super) fn cell_labels(kind: CellKind) -> (&'static str, &'static [&'static str], f32) {
    match kind {
        CellKind::ServiceState => (
            "services.state",
            &[
                "running",
                "stopped",
                "startPending",
                "stopPending",
                "paused",
                "pausePending",
                "continuePending",
            ],
            14.,
        ),
        CellKind::StartType => (
            "services.startType",
            &[
                "auto",
                "autoDelayed",
                "manual",
                "disabled",
                "boot",
                "system",
            ],
            0.,
        ),
        CellKind::ConnectionState => (
            "connections.state",
            &[
                "listen",
                "established",
                "synSent",
                "synReceived",
                "finWait1",
                "finWait2",
                "closeWait",
                "closing",
                "lastAck",
                "timeWait",
                "closed",
            ],
            14.,
        ),
        _ => ("", &[], 0.),
    }
}
