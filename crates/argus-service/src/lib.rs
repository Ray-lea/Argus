//! Opt-in, per-account authorization for a fixed set of local administrative
//! operations. No command execution, arbitrary file writes, or driver access.

mod background;
mod handles;
mod install;
mod network;
mod pipe;
mod protocol;
mod security;
mod server;
pub mod taskmgr;
mod util;

use argus_core::tr;

pub use background::{NetworkClient, RuleRunner};
pub use install::{ServiceStatus, manage, status};
pub use network::NetworkFrame;
pub use protocol::{Operation, Reply};

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
const VERSION: &str = env!("CARGO_PKG_VERSION");
const PROTOCOL: u32 = 4;

/// Handle service/helper modes before starting Tauri, its singleton or sampler.
pub fn run_mode() -> Option<i32> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some(handles::WORKER_ARG) if args.len() == 2 => Some(handles::worker(&args[1]) as i32),
        Some(SERVICE_ARG) if args.len() == 1 => {
            Some(server::dispatch().map_or_else(|e| e as i32, |_| 0))
        }
        Some(INSTALL_ARG) if args.len() == 3 => {
            Some(install::run_helper(&args[1], &args[2]).map_or_else(|e| e as i32, |_| 0))
        }
        Some(REMOVE_ARG) if args.len() == 1 => Some(remove_for_uninstall()),
        Some(SERVICE_ARG | INSTALL_ARG | REMOVE_ARG | handles::WORKER_ARG) => Some(87),
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
    let Some(pid) = install::running_pid().map_err(util::message)? else {
        return Ok(None);
    };
    pipe::request(PIPE_NAME, pid, operation)
        .map(Some)
        .map_err(util::message)
}
