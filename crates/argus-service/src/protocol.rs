use argus_core::actions::{self, ActionError, Priority};
use argus_core::connections;
use argus_core::rules::{self, ProcessRule};
use argus_core::services::{ServiceAction, ServiceCatalog, StartType};
use serde::{Deserialize, Serialize};

pub const MAX_FRAME: usize = 16 * 1024;
pub const MAX_REPLY: usize = 1024 * 1024;

pub(crate) struct Context {
    services: ServiceCatalog,
    network: crate::network::NetworkMonitor,
}

impl Context {
    pub fn new() -> Self {
        Self {
            services: ServiceCatalog::default(),
            network: crate::network::NetworkMonitor::spawn(),
        }
    }

    #[cfg(test)]
    pub fn testing() -> Self {
        Self {
            services: ServiceCatalog::default(),
            network: crate::network::NetworkMonitor::disabled(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase", deny_unknown_fields)]
pub enum Operation {
    Ping,
    RestoreTaskManager,
    NetworkSnapshot,
    FindHandles {
        paths: Vec<String>,
    },
    ApplyRule {
        pid: u32,
        create_time: i64,
        priority: Option<Priority>,
        efficiency: Option<bool>,
    },
    Terminate {
        pid: u32,
        create_time: i64,
    },
    Priority {
        pid: u32,
        create_time: i64,
        priority: Priority,
    },
    Efficiency {
        pid: u32,
        create_time: i64,
        on: bool,
    },
    ControlService {
        name: String,
        action: ServiceAction,
    },
    ServiceStartType {
        name: String,
        start_type: StartType,
    },
    CloseConnection {
        local_address: String,
        local_port: u16,
        remote_address: String,
        remote_port: u16,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub protocol: u32,
    pub operation: Operation,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub enum Reply {
    Pong { version: String, protocol: u32 },
    Done,
    Network(Option<crate::NetworkFrame>),
    Handles(argus_core::handles::HandleReport),
    Priority(Priority),
    Error(ActionError),
}

impl Operation {
    pub fn validate(&self) -> Result<(), ActionError> {
        match self {
            Self::FindHandles { paths } => argus_core::handles::validate_paths(paths)?,
            Self::ApplyRule {
                pid, create_time, ..
            }
            | Self::Terminate { pid, create_time }
            | Self::Priority {
                pid, create_time, ..
            }
            | Self::Efficiency {
                pid, create_time, ..
            } => {
                if *pid <= 4 || *pid == std::process::id() || *create_time <= 0 {
                    return Err(ActionError::Refused);
                }
            }
            Self::ControlService { name, .. } | Self::ServiceStartType { name, .. } => {
                if name.is_empty()
                    || name.len() > 256
                    || name.contains(['\0', '/', '\\'])
                    || name.eq_ignore_ascii_case("ArgusPrivileged")
                    || name.eq_ignore_ascii_case("ArgusPrivilegedDev")
                {
                    return Err(ActionError::Refused);
                }
            }
            Self::CloseConnection {
                local_address,
                remote_address,
                ..
            } => {
                if local_address.parse::<std::net::Ipv4Addr>().is_err()
                    || remote_address.parse::<std::net::Ipv4Addr>().is_err()
                {
                    return Err(ActionError::Refused);
                }
            }
            Self::Ping | Self::NetworkSnapshot | Self::RestoreTaskManager => {}
        }
        Ok(())
    }
}

pub fn execute(operation: Operation, context: &Context, client_pid: u32) -> Reply {
    if let Err(e) = operation.validate() {
        return Reply::Error(e);
    }
    let result = match operation {
        Operation::RestoreTaskManager => crate::taskmgr::restore().map_err(ActionError::Os),
        Operation::FindHandles { paths } => {
            return Reply::Handles(crate::handles::scan_local(paths));
        }
        Operation::NetworkSnapshot => return Reply::Network(context.network.snapshot()),
        Operation::ApplyRule {
            pid,
            create_time,
            priority,
            efficiency,
        } => rules::apply_rule(
            &ProcessRule {
                id: String::new(),
                name: String::new(),
                enabled: true,
                priority,
                efficiency,
            },
            pid,
            create_time,
        ),
        Operation::Ping => {
            return Reply::Pong {
                version: crate::VERSION.into(),
                protocol: crate::PROTOCOL,
            };
        }
        // A client cannot ask the broker to bypass the host's self-protection.
        Operation::Terminate { pid, .. } if pid == client_pid => Err(ActionError::Refused),
        Operation::Terminate { pid, create_time } => actions::terminate(pid, create_time, false),
        Operation::Priority {
            pid,
            create_time,
            priority,
        } => {
            return match actions::set_priority(pid, create_time, priority) {
                Ok(p) => Reply::Priority(p),
                Err(e) => Reply::Error(e),
            };
        }
        Operation::Efficiency {
            pid,
            create_time,
            on,
        } => actions::set_efficiency_mode(pid, create_time, on),
        Operation::ControlService { name, action } => context.services.control(&name, action),
        Operation::ServiceStartType { name, start_type } => {
            context.services.set_start_type(&name, start_type)
        }
        Operation::CloseConnection {
            local_address,
            local_port,
            remote_address,
            remote_port,
        } => connections::close_tcp(&local_address, local_port, &remote_address, remote_port),
    };
    result.map_or_else(Reply::Error, |_| Reply::Done)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handle_queries_validate_paths_and_bound_the_reply() {
        assert!(
            Operation::FindHandles {
                paths: vec![r"\\server\share".into()]
            }
            .validate()
            .is_err()
        );
        assert!(
            Operation::FindHandles {
                paths: vec![r"C:\folder".into()]
            }
            .validate()
            .is_ok()
        );
        let reply = Reply::Handles(argus_core::handles::HandleReport {
            lockers: (0..argus_core::handles::MAX_LOCKERS)
                .map(|pid| argus_core::handles::HandleLocker {
                    pid: pid as u32,
                    create_time: i64::MAX,
                    name: "\u{0001}".repeat(256),
                    path: "\u{0001}".repeat(1024),
                    count: usize::MAX,
                })
                .collect(),
            summary: Default::default(),
        });
        assert!(serde_json::to_vec(&reply).unwrap().len() < MAX_REPLY);
    }

    #[test]
    fn largest_network_snapshot_fits_the_reply_budget() {
        let reply = Reply::Network(Some(crate::NetworkFrame {
            epoch: "0".repeat(38),
            elapsed_us: u64::MAX,
            counters: (0..crate::network::MAX_COUNTERS)
                .map(|_| (u32::MAX, u64::MAX, u64::MAX, u64::MAX, u64::MAX))
                .collect(),
        }));
        assert!(serde_json::to_vec(&reply).unwrap().len() < MAX_REPLY);
    }

    #[test]
    fn rejects_unchecked_processes_and_service_self_control() {
        assert!(
            Operation::Terminate {
                pid: 100,
                create_time: 0
            }
            .validate()
            .is_err()
        );
        assert!(
            Operation::Terminate {
                pid: 4,
                create_time: 1
            }
            .validate()
            .is_err()
        );
        assert!(
            Operation::ControlService {
                name: "argusprivilegedDEV".into(),
                action: ServiceAction::Stop
            }
            .validate()
            .is_err()
        );
        assert!(
            Operation::ControlService {
                name: "Spooler\0other".into(),
                action: ServiceAction::Start
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn rejects_unknown_operations_and_force_flag() {
        assert!(
            serde_json::from_str::<Request>(
                r#"{"protocol":1,"operation":{"op":"execute","command":"cmd"}}"#
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<Operation>(
                r#"{"op":"terminate","pid":5,"create_time":1,"force":true}"#
            )
            .is_err()
        );
    }
}
