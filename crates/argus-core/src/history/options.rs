use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HistoryDetail {
    Full,
    #[default]
    Minute7,
    Minute30,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct RecordGroups {
    pub cpu_extra: bool,
    pub sensors: bool,
    pub memory_detail: bool,
    pub counts: bool,
    pub disks: bool,
    pub nics: bool,
    pub gpus: bool,
    pub battery_detail: bool,
    pub process_disk: bool,
    pub process_network: bool,
    pub process_gpu: bool,
}

impl Default for RecordGroups {
    fn default() -> Self {
        Self {
            cpu_extra: true,
            sensors: true,
            memory_detail: true,
            counts: true,
            disks: true,
            nics: true,
            gpus: true,
            battery_detail: true,
            process_disk: true,
            process_network: true,
            process_gpu: true,
        }
    }
}

impl RecordGroups {
    pub(super) fn includes(self, key: &str) -> bool {
        match key {
            "cpuKernel" | "cpuClock" | "cpuPower" => self.cpu_extra,
            "cpuTemp" | "cpuVoltage" => self.sensors,
            "commitUsed" | "commitLimit" | "memCached" | "memCompressed" | "pagedPool"
            | "nonpagedPool" => self.memory_detail,
            "processes" | "threads" | "handles" => self.counts,
            "batteryTemp" | "batteryVoltage" => self.battery_detail && self.sensors,
            _ if key.starts_with("fan.") => self.sensors,
            _ if key.starts_with("disk.") => {
                self.disks && (self.sensors || !key.ends_with(".temp"))
            }
            _ if key.starts_with("nic.") => self.nics,
            _ if key.starts_with("gpu.") => {
                self.gpus && (self.sensors || !(key.ends_with(".temp") || key.ends_with(".fan")))
            }
            _ => true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryLimit {
    pub days: Option<u32>,
    pub bytes: Option<u64>,
}
impl Default for HistoryLimit {
    fn default() -> Self {
        Self {
            days: Some(30),
            bytes: Some(2 << 30),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Recording {
    pub groups: RecordGroups,
    pub processes: u32,
}
impl Default for Recording {
    fn default() -> Self {
        Self {
            groups: RecordGroups::default(),
            processes: 20,
        }
    }
}

impl Recording {
    pub(super) fn fallback_frame_bytes(self) -> u64 {
        let g = self.groups;
        let system = 94
            + 12 * u64::from(g.cpu_extra)
            + 16 * u64::from(g.sensors)
            + 48 * u64::from(g.memory_detail)
            + 12 * u64::from(g.counts)
            + 40 * u64::from(g.disks)
            + 16 * u64::from(g.nics)
            + 52 * u64::from(g.gpus)
            + 8 * u64::from(g.battery_detail);
        let process = 13
            + 5 * u64::from(g.process_disk)
            + 3 * u64::from(g.process_network)
            + 7 * u64::from(g.process_gpu);
        system
            + process
                * match self.processes {
                    0 => 200,
                    40 => 40,
                    _ => 20,
                }
    }
}

#[derive(Default, Debug, Clone, Copy)]
pub struct HistoryOptions {
    pub limit: HistoryLimit,
    pub detail: HistoryDetail,
    pub recording: Recording,
}
