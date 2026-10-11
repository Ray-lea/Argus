//! Installed memory modules, from the SMBIOS tables (type 17, Memory Device).

use serde::Serialize;
use windows::Win32::System::SystemInformation::{
    FIRMWARE_TABLE_PROVIDER, GetPhysicallyInstalledSystemMemory, GetSystemFirmwareTable,
};

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryInfo {
    /// Physically installed RAM in bytes (includes hardware-reserved memory).
    pub installed: u64,
    /// Configured speed in MT/s.
    pub speed: Option<u32>,
    pub slots_used: u32,
    pub slots_total: u32,
    /// e.g. `DIMM`, `SODIMM`.
    pub form_factor: Option<String>,
    /// e.g. `DDR5`.
    pub kind: Option<String>,
}

const RSMB: FIRMWARE_TABLE_PROVIDER = FIRMWARE_TABLE_PROVIDER(u32::from_be_bytes(*b"RSMB"));

pub fn memory_info() -> MemoryInfo {
    let mut info = MemoryInfo::default();
    let mut kb = 0u64;
    if unsafe { GetPhysicallyInstalledSystemMemory(&mut kb) }.is_ok() {
        info.installed = kb * 1024;
    }

    let size = unsafe { GetSystemFirmwareTable(RSMB, 0, None) };
    if size == 0 {
        return info;
    }
    let mut raw = vec![0u8; size as usize];
    if unsafe { GetSystemFirmwareTable(RSMB, 0, Some(&mut raw)) } == 0 || raw.len() < 8 {
        return info;
    }
    // RawSMBIOSData: 8-byte header, then the structure table.
    let table_len = u32::from_le_bytes([raw[4], raw[5], raw[6], raw[7]]) as usize;
    let table = &raw[8..(8 + table_len).min(raw.len())];

    let mut pos = 0usize;
    while pos + 4 <= table.len() {
        let kind = table[pos];
        let len = table[pos + 1] as usize;
        if len < 4 || pos + len > table.len() {
            break;
        }
        let formatted = &table[pos..pos + len];
        // Skip the string set, which ends with a double NUL.
        let mut end = pos + len;
        while end + 1 < table.len() && !(table[end] == 0 && table[end + 1] == 0) {
            end += 1;
        }
        if kind == 17 && len >= 0x17 {
            read_memory_device(formatted, &mut info);
        }
        if kind == 127 {
            break;
        }
        pos = end + 2;
    }
    info
}

fn read_memory_device(d: &[u8], info: &mut MemoryInfo) {
    let u16_at = |o: usize| d.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]]));
    info.slots_total += 1;
    let size = u16_at(0x0C).unwrap_or(0);
    if size == 0 || size == 0xFFFF {
        return; // empty slot / unknown
    }
    info.slots_used += 1;

    let configured = if d.len() >= 0x22 { u16_at(0x20) } else { None };
    let speed = configured
        .filter(|&s| s != 0 && s != 0xFFFF)
        .or(u16_at(0x15));
    if let Some(s) = speed.filter(|&s| s != 0 && s != 0xFFFF) {
        info.speed = Some(info.speed.map_or(s as u32, |cur| cur.min(s as u32)));
    }
    if info.form_factor.is_none() {
        info.form_factor = match d[0x0E] {
            0x09 => Some("DIMM".into()),
            0x0D => Some("SODIMM".into()),
            0x0F => Some("FB-DIMM".into()),
            0x0B => Some("板载".into()),
            _ => None,
        };
    }
    if info.kind.is_none() {
        info.kind = match d[0x12] {
            0x18 => Some("DDR3".into()),
            0x1A => Some("DDR4".into()),
            0x1B => Some("LPDDR".into()),
            0x1C => Some("LPDDR2".into()),
            0x1D => Some("LPDDR3".into()),
            0x1E => Some("LPDDR4".into()),
            0x22 => Some("DDR5".into()),
            0x23 => Some("LPDDR5".into()),
            _ => None,
        };
    }
}
