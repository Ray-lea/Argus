use argus_core::actions::{self, ActionError, IoPriority, MemoryPriority, Priority};
use argus_core::connections;
use argus_core::rules::{self, ProcessRule};
use argus_core::services::{ServiceAction, ServiceCatalog, StartType};
use serde::{Deserialize, Serialize};

pub const MAX_FRAME: usize = 16 * 1024;
pub const MAX_REPLY: usize = 1024 * 1024;

pub(crate) struct Context {
    services: ServiceCatalog,
    network: crate::network::NetworkMonitor,
    sensors: crate::sensors::SensorMonitor,
    disk: crate::disk::DiskMonitor,
}

impl Context {
    pub fn new() -> Self {
        Self {
            services: ServiceCatalog::default(),
            network: crate::network::NetworkMonitor::spawn(),
            sensors: crate::sensors::SensorMonitor::spawn(),
            disk: crate::disk::DiskMonitor::new(),
        }
    }

    #[cfg(test)]
    pub fn testing() -> Self {
        Self {
            services: ServiceCatalog::default(),
            network: crate::network::NetworkMonitor::disabled(),
            sensors: crate::sensors::SensorMonitor::disabled(),
            disk: crate::disk::DiskMonitor::disabled(),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_readers(
        network: crate::network::NetworkMonitor,
        sensors: crate::sensors::SensorMonitor,
        disk: crate::disk::DiskMonitor,
    ) -> Self {
        Self {
            services: ServiceCatalog::default(),
            network,
            sensors,
            disk,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase", deny_unknown_fields)]
pub enum Operation {
    Ping,
    RestoreTaskManager,
    NetworkSnapshot,
    SensorSnapshot {},
    DiskSnapshot {},
    DiskDetails {
        index: u32,
    },
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
    IoPriority {
        pid: u32,
        create_time: i64,
        priority: IoPriority,
    },
    MemoryPriority {
        pid: u32,
        create_time: i64,
        priority: MemoryPriority,
    },
    Suspend {
        pid: u32,
        create_time: i64,
        suspend: bool,
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
    Pong {
        version: String,
        protocol: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        diagnostics: Option<crate::ServiceDiagnostics>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sensor_driver_version: Option<u32>,
    },
    Done,
    Network(Option<crate::NetworkFrame>),
    Disk(Option<argus_core::disk_process::DiskSnapshot>),
    /// The service's disk query failed: why, in fixed words and codes.
    DiskFailed(argus_core::disk_process::DiskFailure),
    DiskDetails(Box<argus_core::hardware::disk_details::DiskDetails>),
    Sensors(Box<argus_core::hardware::CpuSensors>),
    Handles(argus_core::handles::HandleReport),
    Priority(Priority),
    Error(ActionError),
}

impl Operation {
    pub fn validate(&self) -> Result<(), ActionError> {
        match self {
            Self::DiskDetails { index } => {
                if *index > 4095 {
                    return Err(ActionError::Refused);
                }
            }
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
            }
            | Self::MemoryPriority {
                pid, create_time, ..
            }
            | Self::Suspend {
                pid, create_time, ..
            } => {
                if *pid <= 4 || *pid == std::process::id() || *create_time <= 0 {
                    return Err(ActionError::Refused);
                }
            }
            // High and critical I/O are Windows's own levels.
            Self::IoPriority {
                pid,
                create_time,
                priority,
            } => {
                if *pid <= 4
                    || *pid == std::process::id()
                    || *create_time <= 0
                    || !priority.settable()
                {
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
            Self::Ping
            | Self::NetworkSnapshot
            | Self::SensorSnapshot {}
            | Self::DiskSnapshot {}
            | Self::RestoreTaskManager => {}
        }
        Ok(())
    }
}

pub fn execute(operation: Operation, context: &Context, client_pid: u32) -> Reply {
    if let Err(e) = operation.validate() {
        return Reply::Error(e);
    }
    let result = match operation {
        Operation::DiskDetails { index } => {
            // Bound hardware work across clients; a blocked storage driver must
            // not consume every connection or start concurrent reads of a disk.
            static READING: std::sync::Mutex<()> = std::sync::Mutex::new(());
            let Ok(_reading) = READING.try_lock() else {
                return Reply::Error(ActionError::Refused);
            };
            return Reply::DiskDetails(Box::new(
                argus_core::hardware::disk_details::query_privileged(index),
            ));
        }
        Operation::RestoreTaskManager => crate::taskmgr::restore().map_err(ActionError::Os),
        Operation::FindHandles { paths } => {
            return Reply::Handles(crate::handles::scan_local(paths));
        }
        Operation::NetworkSnapshot => return Reply::Network(context.network.snapshot()),
        Operation::SensorSnapshot {} => {
            return Reply::Sensors(Box::new(context.sensors.snapshot()));
        }
        Operation::DiskSnapshot {} => {
            return match context.disk.snapshot() {
                Some(Ok(frame)) => Reply::Disk(Some(frame)),
                Some(Err(failure)) => Reply::DiskFailed(failure),
                None => Reply::Disk(None),
            };
        }
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
                diagnostics: crate::diagnostics::snapshot(),
                sensor_driver_version: crate::driver::running_device_version(),
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
        Operation::IoPriority {
            pid,
            create_time,
            priority,
        } => actions::set_io_priority(pid, create_time, priority).map(|_| ()),
        Operation::MemoryPriority {
            pid,
            create_time,
            priority,
        } => actions::set_memory_priority(pid, create_time, priority).map(|_| ()),
        // Suspending the host would freeze the window that asked.
        Operation::Suspend { pid, .. } if pid == client_pid => Err(ActionError::Refused),
        Operation::Suspend {
            pid,
            create_time,
            suspend,
        } => actions::set_suspended(pid, create_time, suspend),
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
    fn disk_details_accept_only_a_bounded_disk_number() {
        for input in [
            r#"{"op":"diskDetails","index":0,"command":176}"#,
            r#"{"op":"diskDetails","index":0,"path":"C:"}"#,
            r#"{"op":"diskDetails","index":-1}"#,
        ] {
            assert!(serde_json::from_str::<Operation>(input).is_err());
        }
        assert!(Operation::DiskDetails { index: 0 }.validate().is_ok());
        assert!(Operation::DiskDetails { index: 4096 }.validate().is_err());
    }

    #[test]
    fn suspend_refuses_system_service_unresolved_and_client_targets() {
        let suspend = |pid, create_time| Operation::Suspend {
            pid,
            create_time,
            suspend: true,
        };
        for (pid, create_time) in [(0, 1), (4, 1), (std::process::id(), 1), (1234, 0)] {
            assert!(matches!(
                suspend(pid, create_time).validate(),
                Err(ActionError::Refused)
            ));
        }
        assert!(suspend(1234, 1).validate().is_ok());
        assert!(matches!(
            execute(suspend(1234, 1), &Context::testing(), 1234),
            Reply::Error(ActionError::Refused)
        ));
        let valid = r#"{"op":"suspend","pid":1234,"create_time":1,"suspend":true"#;
        assert!(serde_json::from_str::<Operation>(&format!("{valid}}}")).is_ok());
        assert!(serde_json::from_str::<Operation>(&format!(r#"{valid},"tree":true}}"#)).is_err());
    }

    #[test]
    fn io_and_memory_priority_refuse_reserved_levels_and_protected_targets() {
        let io = |pid, create_time, priority| Operation::IoPriority {
            pid,
            create_time,
            priority,
        };
        let memory = |pid, create_time| Operation::MemoryPriority {
            pid,
            create_time,
            priority: MemoryPriority::VeryLow,
        };
        for (pid, create_time) in [(0, 1), (4, 1), (std::process::id(), 1), (1234, 0)] {
            assert!(matches!(
                io(pid, create_time, IoPriority::Low).validate(),
                Err(ActionError::Refused)
            ));
            assert!(matches!(
                memory(pid, create_time).validate(),
                Err(ActionError::Refused)
            ));
        }
        for priority in [IoPriority::High, IoPriority::Critical] {
            assert!(matches!(
                io(1234, 1, priority).validate(),
                Err(ActionError::Refused)
            ));
        }
        assert!(io(1234, 1, IoPriority::VeryLow).validate().is_ok());
        assert!(memory(1234, 1).validate().is_ok());
        for valid in [
            r#"{"op":"ioPriority","pid":1234,"create_time":1,"priority":"veryLow"}"#,
            r#"{"op":"memoryPriority","pid":1234,"create_time":1,"priority":"belowNormal"}"#,
        ] {
            assert!(serde_json::from_str::<Operation>(valid).is_ok());
        }
        for invalid in [
            r#"{"op":"ioPriority","pid":1234,"create_time":1,"priority":"idle"}"#,
            r#"{"op":"memoryPriority","pid":1234,"create_time":1,"priority":6}"#,
            r#"{"op":"memoryPriority","pid":1234,"create_time":1,"priority":"low","tree":true}"#,
        ] {
            assert!(serde_json::from_str::<Operation>(invalid).is_err());
        }
    }

    #[test]
    fn disk_details_round_trip_preserves_protocol_specific_fields() {
        let details = serde_json::from_value(serde_json::json!({
            "index": 0, "queriedAt": 1, "volumes": [], "viaService": false, "queries": [],
            "ataIdentity": {"model":"Disk", "serialNumber":"0000Factory", "smartSupported":true,
                "smartEnabled":true, "rotationRpm":7200, "solidState":false},
            "smartHealth": {"source":"sat", "predictFailure":null, "attributes":[
                {"id":5,"flags":1,"current":100,"worst":90,"threshold":null,"raw":[1,2,3,4,5,6],"failing":false}
            ],"counters":[]}
        })).unwrap();
        let bytes = serde_json::to_vec(&Reply::DiskDetails(Box::new(details))).unwrap();
        assert!(bytes.len() < MAX_REPLY);
        let Reply::DiskDetails(decoded) = serde_json::from_slice::<Reply>(&bytes).unwrap() else {
            panic!("wrong reply");
        };
        assert_eq!(
            decoded.ata_identity.unwrap().serial_number.as_deref(),
            Some("0000Factory")
        );
        let health = decoded.smart_health.unwrap();
        assert_eq!(health.predict_failure, None);
        assert_eq!(health.attributes[0].raw, [1, 2, 3, 4, 5, 6]);
        assert_eq!(health.attributes[0].threshold, None);
    }

    #[test]
    fn disk_snapshot_is_fixed_and_fits_the_reply_budget() {
        assert!(serde_json::from_str::<Operation>(r#"{"op":"diskSnapshot","pid":42}"#).is_err());
        let reply = Reply::Disk(Some(argus_core::disk_process::DiskSnapshot {
            sampled_at_ms: u64::MAX,
            counters: vec![
                (u32::MAX, i64::MAX, u64::MAX, u64::MAX);
                argus_core::disk_process::MAX_PROCESSES
            ],
        }));
        let bytes = serde_json::to_vec(&reply).unwrap();
        assert!(bytes.len() < MAX_REPLY);
        assert!(serde_json::from_slice::<Reply>(&bytes).is_ok());
        assert!(matches!(
            execute(Operation::DiskSnapshot {}, &Context::testing(), 123),
            Reply::Disk(None)
        ));
        // A failure crosses as its category and code, nothing else.
        use argus_core::disk_process::DiskFailure;
        for failure in [
            DiskFailure::Query(0xC000_0022_u32 as i32),
            DiskFailure::Length,
            DiskFailure::Misaligned,
            DiskFailure::Truncated,
            DiskFailure::ProcessId,
            DiskFailure::TooMany,
        ] {
            let bytes = serde_json::to_vec(&Reply::DiskFailed(failure)).unwrap();
            assert!(matches!(
                serde_json::from_slice::<Reply>(&bytes).unwrap(),
                Reply::DiskFailed(back) if back == failure
            ));
        }
        assert_eq!(
            serde_json::to_string(&Reply::DiskFailed(DiskFailure::Query(-1073741790))).unwrap(),
            r#"{"kind":"diskFailed","value":{"query":-1073741790}}"#
        );
        assert!(serde_json::from_str::<Reply>(r#"{"kind":"diskFailed","value":"path"}"#).is_err());
    }

    #[test]
    fn expanded_cpu_snapshot_stays_within_the_reply_budget() {
        use argus_core::hardware::{
            CoreTemperature, CpuCoreSensors, CpuMetric, CpuMetricKind, CpuSensorSource, CpuSensors,
        };
        let value = CpuCoreSensors {
            core: 255,
            group: 3,
            processor: 63,
            celsius: Some(125.0),
            distance_to_limit: Some(127.0),
            clock_mhz: Some(12000.0),
            effective_mhz: Some(12000.0),
            watts: Some(2000.0),
            vid: Some(2.0),
            multiplier: Some(120.0),
            c0_percent: Some(100.0),
            thermal_throttling: Some(true),
            voltage: Some(2.0),
            smu_c0_percent: Some(100.0),
            c1_percent: Some(100.0),
            c6_percent: Some(100.0),
            os_c1_percent: Some(100.0),
            os_c2_percent: Some(100.0),
            os_c3_percent: Some(100.0),
            efficient: true,
            limits: [
                argus_core::hardware::CpuLimitReason::Thermal,
                argus_core::hardware::CpuLimitReason::Prochot,
                argus_core::hardware::CpuLimitReason::CriticalTemperature,
                argus_core::hardware::CpuLimitReason::Power,
                argus_core::hardware::CpuLimitReason::Current,
                argus_core::hardware::CpuLimitReason::CrossDomain,
            ]
            .into_iter()
            .map(|reason| argus_core::hardware::CpuLimitStatus {
                reason,
                active: true,
                occurred: true,
            })
            .collect(),
        };
        let reply = Reply::Sensors(Box::new(CpuSensors {
            cores: vec![value; 256],
            core_temperatures: vec![
                CoreTemperature {
                    group: 3,
                    processor: 63,
                    celsius: 125.0
                };
                256
            ],
            metrics: vec![
                CpuMetric {
                    kind: CpuMetricKind::SecondCoreVrmCelsius,
                    value: 1234.5678,
                    source: CpuSensorSource::Svi3
                };
                64
            ],
            ..Default::default()
        }));
        let bytes = serde_json::to_vec(&reply).unwrap();
        assert!(bytes.len() < 256 * 1024);
        assert!(bytes.len() < MAX_REPLY);
        assert!(serde_json::from_slice::<Reply>(&bytes).is_ok());
        // Boxing affects in-process layout only; older wire snapshots still decode.
        assert!(matches!(
            serde_json::from_str::<Reply>(r#"{"kind":"sensors","value":{"state":"ready"}}"#).unwrap(),
            Reply::Sensors(frame) if frame.state == argus_core::hardware::SensorState::Ready
        ));
    }

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
            serde_json::from_str::<Operation>(r#"{"op":"sensorSnapshot","register":416}"#).is_err()
        );
        assert!(
            serde_json::from_str::<Operation>(
                r#"{"op":"sensorSnapshot","module":"untrusted.bin"}"#
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<Operation>(r#"{"op":"sensorSnapshot"}"#)
                .unwrap()
                .validate()
                .is_ok()
        );
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
