//! Opt-in, per-account authorization for a fixed set of local administrative
//! operations. Driver access is restricted to fixed, read-only sensor snapshots.

pub mod admin;
mod background;
mod diagnostics;
mod disk;
pub mod driver;
mod handles;
mod install;
mod network;
mod pipe;
mod protocol;
mod security;
mod sensors;
mod server;
pub mod taskmgr;
mod util;

use argus_core::tr;

pub use background::{NetworkClient, RuleRunner};
pub use diagnostics::ServiceDiagnostics;
pub use disk::DiskClient;
pub use install::{ServiceStatus, manage, status};
pub use network::NetworkFrame;
pub use protocol::{Operation, Reply};
pub use sensors::SensorClient;

pub const SERVICE_NAME: &str = if cfg!(debug_assertions) {
    "ArgusPrivilegedDev"
} else {
    "ArgusPrivileged"
};
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
const PROTOCOL: u32 = 9;

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
            | driver::MANAGE_ARG,
        ) => Some(87),
        _ => None,
    }
}

/// Uninstalls the service through the same UAC helper as the settings page,
/// which restores the system task manager first. 0 when nothing is left
/// installed; the uninstaller asks the user what to do otherwise.
fn remove_for_uninstall() -> i32 {
    match status() {
        Ok(s) if s.state == "notInstalled" => 0,
        _ => match manage("uninstall") {
            Ok(s) if s.state == "notInstalled" => 0,
            _ => 1,
        },
    }
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
    let warning = match service {
        Ok(Some(Reply::Handles(mut report))) if report.summary.available => {
            report.summary.via_service = true;
            return report;
        }
        Ok(Some(Reply::Handles(report))) => {
            let error = report
                .summary
                .error
                .unwrap_or_else(|| tr!("service.handles.unavailable"));
            Some(tr!("service.handles.serviceIncomplete", error = error))
        }
        Ok(None) => None,
        Ok(Some(Reply::Error(error))) => Some(match error {
            argus_core::actions::ActionError::Os(message) => {
                tr!("service.handles.serviceIncomplete", error = message)
            }
            _ => tr!("service.handles.serviceIncompleteLocal"),
        }),
        Ok(_) => Some(tr!("service.handles.serviceOutdated")),
        Err(error) => Some(tr!("service.handles.serviceUnavailable", error = error)),
    };
    let mut report = handles::scan_local(paths);
    if let Some(warning) = warning {
        report.summary.error = Some(match report.summary.error.take() {
            Some(error) => tr!("app.join", first = warning, second = error),
            None => warning,
        });
    }
    report
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
                state: "running",
                authorized: true,
                version: None,
                error,
                incompatible: false,
                error_code: None,
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
            Some("os error 109")
        );
        assert_eq!(
            issue(Some(util::message(5))).as_deref(),
            Some("access denied (os error 5)")
        );
    }
}
