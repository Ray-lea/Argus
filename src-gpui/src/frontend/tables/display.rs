//! What a table cell shows as text, and what copying it gives.
use super::*;

/// A process's base priority as its priority class, or as the number Windows
/// reports when it matches no class.
pub(in crate::frontend) fn base_priority(process: &Value) -> Option<String> {
    let value = process["basePriority"].as_i64()?;
    Some(match model::priority_class(process) {
        Some(class) => label(&format!("processes.priority.{class}")),
        None => crate::regional::number!("{value}"),
    })
}

/// Why a base priority shows as a number: it lies between two classes.
pub(in crate::frontend) fn base_priority_hint(process: &Value) -> Option<String> {
    if model::priority_class(process).is_some() {
        return None;
    }
    let base = process["basePriority"].as_i64()?;
    let class = |(value, class): (i64, &str)| {
        format!(
            "{} ({value})",
            label(&format!("processes.priority.{class}"))
        )
    };
    Some(match model::priority_neighbours(base) {
        (Some(below), Some(above)) => tr!(
            "processes.basePriorityBetween",
            below = class(below),
            above = class(above)
        ),
        _ => label("processes.basePriorityCustom"),
    })
}

fn cell(row: &Row, field: &Field) -> String {
    let raw = &row.raw;
    let missing = || label("format.missing");
    match field.kind {
        CellKind::Name => row.title.clone(),
        CellKind::ProcessText => row
            .column_text(field.key)
            .map(|value| columns::shown(field.key, value).into_owned())
            .unwrap_or_else(missing),
        // A process's priority class by name; groups mix them and show none.
        CellKind::Identifier if field.key == "basePriority" => row
            .metric(field.key)
            .and_then(|_| base_priority(&row.members[0]))
            .unwrap_or_else(missing),
        CellKind::Identifier => row
            .metric(field.key)
            .map(|value| crate::regional::number!("{value:.0}"))
            .unwrap_or_else(missing),
        CellKind::Duration => row
            .metric(field.key)
            .map(pages::cpu_time)
            .unwrap_or_else(missing),
        CellKind::Timestamp => row
            .metric(field.key)
            .map(pages::date_time)
            .unwrap_or_else(missing),
        CellKind::AddressSpace => row
            .metric(field.key)
            .map(|value| theme::bytes(value, false))
            .unwrap_or_else(missing),
        CellKind::ProcessBoolean => row
            .metric(field.key)
            .map(|value| {
                label(match (field.key, value > 0.) {
                    ("suspended" | "elevated", true) => "processes.columns.yes",
                    ("suspended" | "elevated", false) => "processes.columns.no",
                    (_, true) => "common.on",
                    (_, false) => "common.off",
                })
            })
            .unwrap_or_else(missing),
        CellKind::Pid => {
            if row.members.len() > 1 || row.section.is_some() {
                String::new()
            } else {
                raw["pid"]
                    .as_u64()
                    .map(|v| v.to_string())
                    .unwrap_or_default()
            }
        }
        CellKind::Count | CellKind::EventRate => row
            .metric(field.key)
            .map(theme::count)
            .unwrap_or_else(missing),
        CellKind::Percent => row
            .metric(field.key)
            .map(|value| format!("{}%", theme::cpu(value)))
            .unwrap_or_else(missing),
        CellKind::Memory => row
            .metric(field.key)
            .map(|value| {
                if columns::byte_count(field.key) {
                    theme::bytes(value, false)
                } else {
                    theme::memory(value)
                }
            })
            .unwrap_or_else(missing),
        CellKind::Rate | CellKind::NetworkRate => row
            .metric(field.key)
            .map(|v| theme::bytes(v, true))
            .unwrap_or_else(missing),
        CellKind::Power => row
            .metric(field.key)
            .map(columns::power_label)
            .unwrap_or_else(missing),
        _ => entry_cell(raw, field),
    }
}

/// What a service, startup or connection cell shows, read from the entry
/// alone (the lists' widths measure entries, not rows); empty for a name or
/// a process reading.
pub(super) fn entry_cell(raw: &Value, field: &Field) -> String {
    let missing = || label("format.missing");
    match field.kind {
        CellKind::Dim | CellKind::Mono | CellKind::MonoDim => {
            let value = &raw[field.key];
            value.as_str().map(str::to_owned).unwrap_or_else(|| {
                if !value.is_null() {
                    value.to_string()
                } else if field.key == "publisher" {
                    missing()
                } else {
                    String::new()
                }
            })
        }
        // A stopped service has no process.
        CellKind::ServicePid => raw["pid"]
            .as_u64()
            .filter(|pid| *pid != 0)
            .map(|v| v.to_string())
            .unwrap_or_default(),
        CellKind::ServiceState => label(&format!("services.state.{}", text(raw, "state"))),
        CellKind::StartType => {
            if raw["startType"].is_null() {
                missing()
            } else if !raw["pendingStartType"].is_null() {
                // Written to the registry; the SCM still runs the old value.
                pending_start_type(text(raw, "pendingStartType"))
            } else {
                label(&format!("services.startType.{}", text(raw, "startType")))
            }
        }
        CellKind::Enabled => label(if text(raw, "blocked") == "unavailable" {
            "startup.stateUnavailable"
        } else if boolean(raw, "enabled") {
            "common.enabled"
        } else {
            "common.disabled"
        }),
        CellKind::StartupLocation => label(&format!("startup.location.{}", text(raw, "location"))),
        CellKind::Protocol => text(raw, "protocol").to_uppercase(),
        CellKind::Port | CellKind::PortDim => raw[field.key]
            .as_u64()
            .map(|v| v.to_string())
            .unwrap_or_default(),
        CellKind::ConnectionState => {
            let state = text(raw, "state");
            if state.is_empty() {
                String::new()
            } else {
                label(&format!("connections.state.{state}"))
            }
        }
        _ => String::new(),
    }
}

/// What copying `row`'s cell in `field` gives: its text, with what the cell
/// draws apart from the text, such as an IPv6 connection's "v6" tag.
pub(super) fn copied_cell(row: &Row, field: &Field, bits: bool) -> String {
    let text = display_cell(row, field, bits);
    if field.kind == CellKind::Protocol {
        protocol_copy(text, boolean(&row.raw, "ipv6"))
    } else {
        text
    }
}

/// A protocol as copied: "TCPv6" for IPv6, as the protocol filter names it.
fn protocol_copy(protocol: String, ipv6: bool) -> String {
    if ipv6 && !protocol.is_empty() {
        format!("{protocol}v6")
    } else {
        protocol
    }
}

pub(super) fn display_cell(row: &Row, field: &Field, bits: bool) -> String {
    if field.kind == CellKind::NetworkRate {
        row.metric(field.key)
            .map(|value| {
                if value < 1. {
                    "0".into()
                } else {
                    theme::network(value, bits)
                }
            })
            .unwrap_or_else(|| label("format.missing"))
    } else {
        cell(row, field)
    }
}
/// A start type written to the registry that takes effect after a restart.
pub(super) fn pending_start_type(start_type: &str) -> String {
    argus_core::i18n::format(
        "services.startType.pending",
        &[(
            "type",
            &label(&format!("services.startType.{start_type}")) as &dyn std::fmt::Display,
        )],
    )
}
impl Desktop {
    /// The name of the current table's column `key`, and `row`'s cell in it
    /// as the table shows it.
    pub(in crate::frontend) fn cell_copy(
        &self,
        row: &Row,
        key: &str,
        cx: &App,
    ) -> Option<(String, String)> {
        let delegate = self.exportable_table(cx)?;
        let field = delegate.fields.iter().find(|field| field.key == key)?;
        Some((field.label.clone(), copied_cell(row, field, delegate.bits)))
    }
    /// Every column of the current table with `row`'s cell in it, as the
    /// table shows them.
    pub(in crate::frontend) fn row_cells(&self, row: &Row, cx: &App) -> Vec<(String, String)> {
        let Some(delegate) = self.exportable_table(cx) else {
            return Vec::new();
        };
        delegate
            .fields
            .iter()
            .map(|field| (field.label.clone(), copied_cell(row, field, delegate.bits)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    // Named imports keep the glob-imported GPUI `test` macro out of scope.
    use super::protocol_copy;

    #[test]
    fn copied_protocols_keep_the_ipv6_tag() {
        assert_eq!(protocol_copy("TCP".into(), true), "TCPv6");
        assert_eq!(protocol_copy("UDP".into(), false), "UDP");
        assert_eq!(protocol_copy(String::new(), true), "");
    }
}
