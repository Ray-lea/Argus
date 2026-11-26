//! Fixtures shared by the history tests, and the tests that exercise
//! several parts together.
mod damage;
mod events;
mod retention;
use super::*;
use crate::hardware::Hardware;
use crate::process::ProcessRow;
use crate::system::SystemStats;

pub(super) fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("argus-history-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    prepare_directory(&dir, None).unwrap();
    dir
}

pub(super) fn proc(pid: u32, name: &str, cpu: f32, mem: u64) -> ProcessRow {
    ProcessRow {
        pid,
        create_time: 1,
        name: name.into(),
        cpu,
        private_ws: mem,
        working_set: mem,
        commit: mem,
        gpu_dedicated: Some(0),
        gpu_shared: Some(0),
        threads: 1,
        handles: 1,
        session_id: 1,
        base_priority: 8,
        ..ProcessRow::default()
    }
}

pub(super) fn snapshot(ts: i64, cpu: f32, procs: Vec<ProcessRow>) -> Snapshot {
    Snapshot {
        seq: 0,
        timestamp: ts,
        system: SystemStats {
            cpu,
            mem_used: 8 << 30,
            mem_total: 16 << 30,
            ..Default::default()
        },
        hardware: Hardware::default(),
        processes: procs,
        sessions: Vec::new(),
        leaks: Vec::new(),
        process_network: false,
    }
}

pub(super) fn battery(percent: f32, rate: Option<i32>) -> crate::hardware::BatteryStats {
    crate::hardware::BatteryStats {
        info: std::sync::Arc::new(crate::hardware::BatteryInfo {
            id: "0".to_string(),
            name: String::new(),
            manufacturer: String::new(),
            serial: String::new(),
            chemistry: String::new(),
            designed_capacity: None,
            full_capacity: None,
            cycle_count: None,
            manufactured: None,
        }),
        percent,
        capacity: None,
        rate,
        voltage: None,
        state: if rate.is_some_and(|r| r < 0) {
            "discharging"
        } else {
            "charging"
        },
        plugged_in: false,
        critical: false,
        remaining: None,
        temperature: None,
    }
}
