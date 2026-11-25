//! Opt-in, per-account authorization for a fixed set of local administrative
//! operations. Driver access is restricted to fixed, read-only sensor snapshots.

pub mod admin;
mod background;
mod cleanup;
mod diagnostics;
mod disk;
pub mod driver;
pub mod folder;
mod handles;
mod install;
mod layout;
mod network;
mod pipe;
mod protocol;
mod security;
mod sensors;
mod server;
mod snapshot;
pub mod taskmgr;
mod util;

use argus_core::tr;

pub use background::{NetworkClient, RuleRunner};
pub use diagnostics::ServiceDiagnostics;
pub use disk::DiskClient;
pub use install::{Damage, ServiceStatus, manage, status};
pub use network::NetworkFrame;
pub use protocol::{Operation, Reply};
pub use sensors::SensorClient;

pub const SERVICE_NAME: &str = if cfg!(debug_assertions) {
    "ArgusPrivilegedDev"
} else {
    "ArgusPrivileged"
};

/// Whether `name` is the privileged service of either build, which only
/// its own install and uninstall change.
pub fn is_own_service(name: &str) -> bool {
    name.eq_ignore_ascii_case("ArgusPrivileged") || name.eq_ignore_ascii_case("ArgusPrivilegedDev")
}
const PIPE_NAME: &str = if cfg!(debug_assertions) {
    r"\\.\pipe\Argus.Privileged.Dev.v1"
} else {
    r"\\.\pipe\Argus.Privileged.v1"
};
const SERVICE_ARG: &str = "--argus-service";
const INSTALL_ARG: &str = "--argus-service-manage";
/// Run by the NSIS uninstaller once the user agreed to remove the service too.
const REMOVE_ARG: &str = "--argus-service-remove";
/// Run by the NSIS installer after it replaced argus.exe: updates an
/// installed service in the same elevation.
const REFRESH_ARG: &str = "--argus-service-refresh";
const VERSION: &str = env!("CARGO_PKG_VERSION");
const PROTOCOL: u32 = 13;

/// Handle service/helper modes before starting the native UI, its singleton or sampler.
pub fn run_mode() -> Option<i32> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some(driver::MANAGE_ARG) if args.len() == 1 => Some(diagnostics::helper(
            "sensor driver helper",
            driver::run_helper,
        )),
        Some(handles::WORKER_ARG) if args.len() == 2 => Some(handles::worker(&args[1]) as i32),
        Some(SERVICE_ARG) if args.len() == 1 => {
            argus_core::diagnostics::capture_helper_failures();
            Some(server::dispatch().map_or_else(|e| e as i32, |_| 0))
        }
        Some(INSTALL_ARG) if args.len() == 3 => {
            Some(diagnostics::helper("privileged service helper", || {
                install::run_helper(&args[1], &args[2])
            }))
        }
        Some(REMOVE_ARG) if args.len() == 1 => Some(remove_for_uninstall()),
        // Not a diagnostics::helper: a report it saved would recreate the folder.
        Some(driver::CLEANUP_ARG) if args.len() == 1 => {
            Some(driver::remove_setup_folder().map_or_else(|e| e as i32, |_| 0))
        }
        Some(REFRESH_ARG) if args.len() == 1 => Some(diagnostics::helper(
            "installer service refresh",
            install::refresh,
        )),
        Some(
            SERVICE_ARG
            | INSTALL_ARG
            | REMOVE_ARG
            | REFRESH_ARG
            | handles::WORKER_ARG
            | driver::MANAGE_ARG
            | driver::CLEANUP_ARG,
        ) => Some(87),
        _ => None,
    }
}

/// Uninstalls the service through the same UAC helper as the settings page,
/// which restores the system task manager first. 0 when nothing is left
/// installed; the uninstaller asks the user what to do otherwise.
fn remove_for_uninstall() -> i32 {
    match status() {
        // Nothing registered, though files may be left: another program (a
        // security product, say) deleted the registration. They go without a
        // question, and never hold up the uninstall: the uninstaller already
        // runs elevated, and a removal deletes only by name.
        Ok(s) if s.state == "notInstalled" => {
            let _ = crate::util::stage(
                "privileged service leftover removal",
                install::run_helper("uninstall", ""),
            );
            0
        }
        _ => match manage("uninstall") {
            // Deleted, with its files: a registration that another program's
            // handle still keeps goes when that closes.
            Ok(s) if matches!(s.state, "notInstalled" | "deletePending") => 0,
            _ => 1,
        },
    }
}

/// Open-files queries are read-only; fall back to a local worker like
/// [`find_handle_lockers`] when the privileged service is unavailable.
pub fn find_process_open_files(
    pid: u32,
    create_time: i64,
) -> argus_core::handles::ProcessOpenFilesReport {
    let service = install::running_pid()
        .map_err(util::message)
        .and_then(|running| {
            running
                .map(|running| {
                    pipe::request_with_timeout(
                        PIPE_NAME,
                        running,
                        Operation::FindProcessOpenFiles { pid, create_time },
                        std::time::Duration::from_secs(20),
                    )
                    .map_err(util::message)
                })
                .transpose()
        });
    let fallback = match service {
        Ok(Some(Reply::ProcessOpenFiles(mut report))) if report.summary.available => {
            report.summary.via_service = true;
            return report;
        }
        Ok(Some(Reply::ProcessOpenFiles(report))) => Fallback::ServiceFailed(
            report
                .summary
                .error
                .unwrap_or_else(|| tr!("service.handles.unavailable")),
        ),
        Ok(None) => Fallback::NoService,
        Ok(Some(Reply::Error(argus_core::actions::ActionError::Os(message)))) => {
            Fallback::ServiceFailed(message)
        }
        Ok(Some(Reply::Error(_))) => Fallback::Note(tr!("service.handles.serviceIncompleteLocal")),
        Ok(_) => Fallback::Note(tr!("service.handles.serviceOutdated")),
        Err(error) => Fallback::Note(tr!("service.handles.serviceUnavailable", error = error)),
    };
    let mut report = handles::scan_process_local(pid, create_time);
    report.summary.service_asked = !matches!(fallback, Fallback::NoService);
    report.summary.error = fallback.error(report.summary.error.take());
    report
}

/// Handle queries are read-only, so a failed service query can safely fall
/// back to the current account. Keep its failure visible with partial results.
pub fn find_handle_lockers(paths: Vec<String>) -> argus_core::handles::HandleReport {
    let service = install::running_pid()
        .map_err(util::message)
        .and_then(|pid| {
            pid.map(|pid| {
                pipe::request_with_timeout(
                    PIPE_NAME,
                    pid,
                    Operation::FindHandles {
                        paths: paths.clone(),
                    },
                    std::time::Duration::from_secs(20),
                )
                .map_err(util::message)
            })
            .transpose()
        });
    let fallback = match service {
        Ok(Some(Reply::Handles(mut report))) if report.summary.available => {
            report.summary.via_service = true;
            return report;
        }
        Ok(Some(Reply::Handles(report))) => Fallback::ServiceFailed(
            report
                .summary
                .error
                .unwrap_or_else(|| tr!("service.handles.unavailable")),
        ),
        Ok(None) => Fallback::NoService,
        Ok(Some(Reply::Error(argus_core::actions::ActionError::Os(message)))) => {
            Fallback::ServiceFailed(message)
        }
        Ok(Some(Reply::Error(_))) => Fallback::Note(tr!("service.handles.serviceIncompleteLocal")),
        Ok(_) => Fallback::Note(tr!("service.handles.serviceOutdated")),
        Err(error) => Fallback::Note(tr!("service.handles.serviceUnavailable", error = error)),
    };
    let mut report = handles::scan_local(paths);
    // The message explains the service; the interface does not also suggest
    // enabling it.
    report.summary.service_asked = !matches!(fallback, Fallback::NoService);
    report.summary.error = fallback.error(report.summary.error.take());
    report
}

/// Why handles were scanned locally rather than by the service.
enum Fallback {
    NoService,
    /// Shown with the local results.
    Note(String),
    /// The service's scan failed with this error.
    ServiceFailed(String),
}

impl Fallback {
    /// The message for a local scan that ended with `local`.
    fn error(self, local: Option<String>) -> Option<String> {
        let note = match self {
            Self::NoService => return local,
            Self::Note(note) => note,
            // Both scans read the same system tables, so they often fail the
            // same way: say it once.
            Self::ServiceFailed(error) if local.as_ref() == Some(&error) => {
                return Some(tr!("service.handles.bothIncomplete", error = error));
            }
            Self::ServiceFailed(error) => tr!("service.handles.serviceIncomplete", error = error),
        };
        Some(match local {
            Some(local) => tr!("app.join", first = note, second = local),
            None => note,
        })
    }
}

/// On-demand disk reads reuse only an already-running, authenticated service.
/// Basic identity and NVMe data remain available without it.
pub fn disk_details(index: u32) -> argus_core::hardware::disk_details::DiskDetails {
    use argus_core::hardware::disk_details::{self, DiskQuery, DiskQueryState, DiskQueryStatus};
    let mut local = disk_details::query(index);
    if !local.needs_smart_supplement() {
        return local;
    }
    let state = match route(Operation::DiskDetails { index }) {
        Ok(Routed::Replied(reply)) => match *reply {
            Reply::DiskDetails(mut details) if details.index == index => {
                details.via_service = true;
                details.refresh_ata_health();
                return *details;
            }
            _ => DiskQueryState::Unavailable,
        },
        Ok(Routed::Absent) => DiskQueryState::ServiceUnavailable,
        Ok(Routed::Outdated) => DiskQueryState::ServiceOutdated,
        Err(_) => DiskQueryState::Unavailable,
    };
    local.queries.push(DiskQueryStatus {
        query: DiskQuery::Service,
        state,
        win32_error: None,
    });
    local
}

/// None means no service is installed. Once connected, never retry a mutation
/// locally: a lost reply does not tell us whether the operation already ran.
pub fn request(operation: Operation) -> Result<Option<Reply>, String> {
    let Some(pid) = util::stage(
        "privileged operation service lookup",
        install::running_pid(),
    )
    .map_err(util::message)?
    else {
        return Ok(None);
    };
    pipe::request(PIPE_NAME, pid, operation)
        .map(Some)
        .map_err(util::message)
}

/// How the privileged service took an operation of the host.
pub enum Routed {
    /// No service runs for this account: the host acts with its own rights.
    Absent,
    /// The service speaks another protocol and refused the operation without
    /// running it: the host may act with its own rights; where those fall
    /// short, the service needs an update.
    Outdated,
    Replied(Box<Reply>),
}

/// Like `request`, telling an outdated service from a failed one. The only
/// error after which the host may still act itself is `Outdated`.
pub fn route(operation: Operation) -> Result<Routed, String> {
    let Some(pid) = install::running_pid().map_err(util::message)? else {
        return Ok(Routed::Absent);
    };
    match pipe::request(PIPE_NAME, pid, operation) {
        Ok(reply) => Ok(Routed::Replied(Box::new(reply))),
        Err(util::INCOMPATIBLE) => Ok(Routed::Outdated),
        Err(e) => Err(util::message(e)),
    }
}

/// What went wrong according to `status`, in fixed words for the diagnostic
/// log (its error is translated). None without an error, and when the only
/// one is that another account owns the service, which the log says anyway.
/// Prefer the original numeric result over localized text. The text fallback
/// supports callers that construct a status without a native result.
pub fn status_issue(status: &ServiceStatus) -> Option<std::borrow::Cow<'static, str>> {
    if let Some(code) = status.error_code {
        return (code != util::OTHER_OWNER).then(|| argus_core::diagnostics::code(code).into());
    }
    let error = status.error.as_deref()?;
    if error == util::message(util::OTHER_OWNER) {
        None
    } else if error == util::message(util::INCOMPATIBLE) {
        Some("incompatible protocol".into())
    } else if error == util::message(1460) {
        Some("timeout".into())
    } else {
        Some(argus_core::diagnostics::failure(error))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_issue_names_the_cause_in_fixed_words() {
        let issue = |error: Option<String>| {
            status_issue(&ServiceStatus {
                authorized: true,
                error,
                ..ServiceStatus::new("running")
            })
            .map(|s| s.into_owned())
        };
        assert_eq!(issue(None), None);
        assert_eq!(issue(Some(util::message(util::OTHER_OWNER))), None);
        assert_eq!(
            issue(Some(util::message(util::INCOMPATIBLE))).as_deref(),
            Some("incompatible protocol")
        );
        assert_eq!(issue(Some(util::message(1460))).as_deref(), Some("timeout"));
        assert_eq!(
            issue(Some(util::message(109))).as_deref(),
            Some("pipe closed (os error 109)")
        );
        assert_eq!(
            issue(Some(util::message(5))).as_deref(),
            Some("access denied (os error 5)")
        );
    }

    #[test]
    fn a_failure_shared_by_the_service_and_the_local_scan_is_shown_once() {
        let error = "0xC0000004".to_owned();
        let once = Fallback::ServiceFailed(error.clone())
            .error(Some(error.clone()))
            .unwrap();
        assert_eq!(once.matches(&error).count(), 1);
        let both = Fallback::ServiceFailed(error.clone())
            .error(Some("0xC0000022".into()))
            .unwrap();
        assert!(both.contains(&error) && both.contains("0xC0000022"));
        assert_eq!(
            Fallback::ServiceFailed(error.clone())
                .error(None)
                .unwrap()
                .matches(&error)
                .count(),
            1
        );
        assert_eq!(Fallback::NoService.error(None), None);
        assert_eq!(Fallback::NoService.error(Some(error.clone())), Some(error));
    }
}
