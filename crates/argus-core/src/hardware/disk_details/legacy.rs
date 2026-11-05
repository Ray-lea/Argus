//! Fixed, read-only Windows/ATA/SAT/SCSI health queries. No command bytes or
//! device paths come from IPC; callers supply only a PhysicalDrive number.
use std::mem::{offset_of, size_of};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use windows::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE, HANDLE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::IO::DeviceIoControl;
use windows::core::PCWSTR;

use super::io::{Failure, Result, record};
use super::{AtaAttribute, DiskDetails, DiskQuery, DiskQueryState, ScsiCounter, ata, scsi};
use crate::util::{OwnedHandle, wide};

const PREDICT_FAILURE: u32 = 0x002d1100;
const ATA_PASS_THROUGH: u32 = 0x0004d02c;
const SCSI_PASS_THROUGH: u32 = 0x0004d004;
const SENSE_SIZE: usize = 64;
const BUDGET: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SmartSource {
    Windows,
    Ata,
    Sat,
    Scsi,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SmartHealth {
    pub source: SmartSource,
    pub predict_failure: Option<bool>,
    pub temperature_celsius: Option<f32>,
    pub temperature_limit_celsius: Option<f32>,
    pub attributes: Vec<AtaAttribute>,
    pub counters: Vec<ScsiCounter>,
    pub scsi_asc: Option<u8>,
    pub scsi_ascq: Option<u8>,
}

impl SmartHealth {
    fn new(source: SmartSource) -> Self {
        Self {
            source,
            predict_failure: None,
            temperature_celsius: None,
            temperature_limit_celsius: None,
            attributes: Vec::new(),
            counters: Vec::new(),
            scsi_asc: None,
            scsi_ascq: None,
        }
    }
    fn prediction(&mut self, prediction: bool) {
        self.predict_failure = Some(self.predict_failure.unwrap_or(false) || prediction);
    }
}

pub(super) fn prediction(handle: HANDLE, details: &mut DiskDetails) {
    let value = super::io::control(handle, PREDICT_FAILURE, &[], 516).and_then(|data| {
        if data.len() < 516 {
            return Err(Failure::invalid());
        }
        let mut health = SmartHealth::new(SmartSource::Windows);
        health.prediction(u32::from_le_bytes(data[..4].try_into().unwrap()) != 0);
        // SCSI vendor-specific bytes do not have the ATA attribute layout.
        if details
            .identity
            .as_ref()
            .is_some_and(|i| matches!(i.bus.as_str(), "ata" | "sata"))
        {
            health.attributes = ata::attributes(&data[4..]).unwrap_or_default();
        }
        Ok(health)
    });
    details.smart_health = record(details, DiskQuery::FailurePrediction, value);
}

pub(super) fn supplement(details: &mut DiskDetails) {
    if !details.needs_smart_supplement() {
        return;
    }
    let path = wide(&format!(r"\\.\PhysicalDrive{}", details.index));
    let opened = unsafe {
        CreateFileW(
            PCWSTR(path.as_ptr()),
            GENERIC_READ.0 | GENERIC_WRITE.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            None,
        )
    }
    .map(OwnedHandle)
    .map_err(Failure::native);
    let Some(handle) = record(details, DiskQuery::SmartAccess, opened) else {
        return;
    };
    let start = Instant::now();
    let bus = details
        .identity
        .as_ref()
        .map(|i| i.bus.as_str())
        .unwrap_or("");
    let transports: &[Transport] = if matches!(bus, "ata" | "sata") {
        &[Transport::Ata, Transport::Sat16, Transport::Sat12]
    } else {
        &[Transport::Sat16, Transport::Sat12]
    };
    let mut identified = Err(unsupported());
    let mut selected = None;
    for transport in transports {
        if start.elapsed() >= BUDGET {
            break;
        }
        identified = ata_command(handle.0, *transport, AtaRead::Identify)
            .and_then(|reply| ata::identify(&reply.data).ok_or_else(Failure::invalid));
        if identified.is_ok() {
            selected = Some(*transport);
            break;
        }
    }
    details.ata_identity = record(details, DiskQuery::AtaIdentity, identified);
    if let Some(transport) = selected {
        let identity = details.ata_identity.as_ref().unwrap();
        if let Some(ssd) = identity.solid_state {
            details.solid_state = Some(ssd);
        }
        // Reading must not implicitly enable SMART on a disabled drive.
        if identity.smart_supported == Some(false) || identity.smart_enabled == Some(false) {
            record::<()>(details, DiskQuery::AtaAttributes, Err(unsupported()));
            return;
        }
        let mut health = details
            .smart_health
            .take()
            .unwrap_or_else(|| SmartHealth::new(SmartSource::Ata));
        health.source = if matches!(transport, Transport::Ata) {
            SmartSource::Ata
        } else {
            SmartSource::Sat
        };
        let read = |command| {
            if start.elapsed() >= BUDGET {
                Err(unavailable())
            } else {
                ata_command(handle.0, transport, command)
            }
        };
        let attributes = read(AtaRead::Attributes)
            .and_then(|reply| ata::attributes(&reply.data).ok_or_else(Failure::invalid));
        if let Some(attributes) = record(details, DiskQuery::AtaAttributes, attributes) {
            health.attributes = attributes;
            let thresholds = read(AtaRead::Thresholds).and_then(|reply| {
                ata::thresholds(&reply.data, &mut health.attributes).ok_or_else(Failure::invalid)
            });
            record(details, DiskQuery::AtaThresholds, thresholds);
        }
        let status = read(AtaRead::Status).and_then(|reply| {
            let registers = reply.registers.ok_or_else(Failure::invalid)?;
            match (registers[3], registers[4]) {
                (0x4f, 0xc2) => Ok(false),
                (0xf4, 0x2c) => Ok(true),
                _ => Err(Failure::invalid()),
            }
        });
        if let Some(failed) = record(details, DiskQuery::AtaStatus, status) {
            health.prediction(failed);
        }
        if health.attributes.iter().any(|attribute| attribute.failing) {
            health.prediction(true);
        }
        if health.predict_failure.is_some() || !health.attributes.is_empty() {
            details.smart_health = Some(health);
        }
        return;
    }
    if start.elapsed() >= BUDGET {
        return;
    }
    let serial = scsi_read(handle.0, &[0x12, 1, 0x80, 0, 252, 0], 252)
        .and_then(|reply| scsi::serial(&reply.data).ok_or_else(Failure::invalid));
    details.scsi_identity = record(details, DiskQuery::ScsiIdentity, serial);
    let mut health = details
        .smart_health
        .take()
        .unwrap_or_else(|| SmartHealth::new(SmartSource::Scsi));
    let mut read_any = false;
    for (page, query) in [
        (0x2f, DiskQuery::ScsiHealth),
        (0x0d, DiskQuery::ScsiTemperature),
        (3, DiskQuery::ScsiReadErrors),
        (2, DiskQuery::ScsiWriteErrors),
    ] {
        if start.elapsed() >= BUDGET {
            break;
        }
        let answer = scsi_read(handle.0, &[0x4d, 0, 0x40 | page, 0, 0, 0, 0, 2, 0, 0], 512)
            .and_then(|reply| {
                match page {
                    0x2f => {
                        let (asc, ascq, temp) =
                            scsi::health(&reply.data).ok_or_else(Failure::invalid)?;
                        health.scsi_asc = Some(asc);
                        health.scsi_ascq = Some(ascq);
                        // Nonzero ASC is surfaced, not classified as a quiet healthy drive.
                        health.prediction(asc != 0);
                        health.temperature_celsius = temp;
                    }
                    0x0d => {
                        let (temp, limit) =
                            scsi::temperatures(&reply.data).ok_or_else(Failure::invalid)?;
                        if temp.is_some() {
                            health.temperature_celsius = temp;
                        }
                        health.temperature_limit_celsius = limit;
                    }
                    _ => health.counters.extend(
                        scsi::counters(&reply.data, page == 2).ok_or_else(Failure::invalid)?,
                    ),
                }
                Ok(())
            });
        read_any |= record(details, query, answer).is_some();
    }
    if read_any {
        health.source = SmartSource::Scsi;
    }
    if read_any || health.predict_failure.is_some() {
        details.smart_health = Some(health);
    }
}

fn unsupported() -> Failure {
    Failure {
        state: DiskQueryState::Unsupported,
        code: None,
    }
}
fn unavailable() -> Failure {
    Failure {
        state: DiskQueryState::Unavailable,
        code: None,
    }
}

#[derive(Clone, Copy)]
enum Transport {
    Ata,
    Sat16,
    Sat12,
}
#[derive(Clone, Copy)]
enum AtaRead {
    Identify,
    Attributes,
    Thresholds,
    Status,
}
impl AtaRead {
    fn task_file(self) -> [u8; 8] {
        match self {
            Self::Identify => [0, 1, 0, 0, 0, 0, 0xec, 0],
            Self::Attributes => [0xd0, 1, 0, 0x4f, 0xc2, 0, 0xb0, 0],
            Self::Thresholds => [0xd1, 1, 0, 0x4f, 0xc2, 0, 0xb0, 0],
            Self::Status => [0xda, 0, 0, 0x4f, 0xc2, 0, 0xb0, 0],
        }
    }
    fn length(self) -> usize {
        if matches!(self, Self::Status) { 0 } else { 512 }
    }
}

// SDK layouts used only for checked offsets; bytes are initialized separately,
// so Rust padding is never copied into a device request.
#[repr(C)]
struct AtaHeader {
    length: u16,
    flags: u16,
    path: u8,
    target: u8,
    lun: u8,
    reserved: u8,
    transfer: u32,
    timeout: u32,
    reserved_long: u32,
    offset: usize,
    previous: [u8; 8],
    current: [u8; 8],
}
#[repr(C)]
struct ScsiHeader {
    length: u16,
    status: u8,
    path: u8,
    target: u8,
    lun: u8,
    cdb_length: u8,
    sense_length: u8,
    data_in: u8,
    transfer: u32,
    timeout: u32,
    offset: usize,
    sense_offset: u32,
    cdb: [u8; 16],
}
struct Answer {
    data: Vec<u8>,
    registers: Option<[u8; 8]>,
}

fn ata_command(handle: HANDLE, transport: Transport, command: AtaRead) -> Result<Answer> {
    let task = command.task_file();
    let length = command.length();
    if !matches!(transport, Transport::Ata) {
        return scsi_read(handle, &sat_cdb(transport, task, length), length);
    }
    let payload = 64usize;
    let mut buffer = vec![0; payload + length];
    buffer[..2].copy_from_slice(&(size_of::<AtaHeader>() as u16).to_le_bytes());
    buffer[2..4].copy_from_slice(&(if length == 0 { 1u16 } else { 3u16 }).to_le_bytes());
    put32(&mut buffer, offset_of!(AtaHeader, transfer), length as u32);
    put32(&mut buffer, offset_of!(AtaHeader, timeout), 2);
    put_size(
        &mut buffer,
        offset_of!(AtaHeader, offset),
        if length == 0 { 0 } else { payload },
    );
    let at = offset_of!(AtaHeader, current);
    buffer[at..at + 8].copy_from_slice(&task);
    let returned = exchange(
        handle,
        ATA_PASS_THROUGH,
        &mut buffer,
        size_of::<AtaHeader>(),
    )?;
    if returned < size_of::<AtaHeader>() || buffer[at + 6] & 0x21 != 0 {
        return Err(unavailable());
    }
    if length != 0
        && (returned < payload + length
            || get32(&buffer, offset_of!(AtaHeader, transfer)) < length as u32
            || get_size(&buffer, offset_of!(AtaHeader, offset)) != payload)
    {
        return Err(Failure::invalid());
    }
    Ok(Answer {
        data: buffer[payload..payload + length].to_vec(),
        registers: Some(buffer[at..at + 8].try_into().unwrap()),
    })
}

fn sat_cdb(transport: Transport, task: [u8; 8], length: usize) -> Vec<u8> {
    let mut cdb = vec![
        0;
        if matches!(transport, Transport::Sat12) {
            12
        } else {
            16
        }
    ];
    cdb[1] = if length == 0 { 3 << 1 } else { 4 << 1 };
    cdb[2] = if length == 0 { 0x20 } else { 0x2e };
    if cdb.len() == 12 {
        cdb[0] = 0xa1;
        cdb[3..10].copy_from_slice(&task[..7]);
    } else {
        cdb[0] = 0x85;
        for (at, value) in [4, 6, 8, 10, 12, 13, 14].into_iter().zip(task) {
            cdb[at] = value;
        }
    }
    cdb
}

fn scsi_read(handle: HANDLE, cdb: &[u8], length: usize) -> Result<Answer> {
    let sense_at = size_of::<ScsiHeader>();
    let payload = (sense_at + SENSE_SIZE).next_multiple_of(64);
    let mut buffer = vec![0; payload + length];
    buffer[..2].copy_from_slice(&(sense_at as u16).to_le_bytes());
    buffer[offset_of!(ScsiHeader, cdb_length)] = cdb.len() as u8;
    buffer[offset_of!(ScsiHeader, sense_length)] = SENSE_SIZE as u8;
    buffer[offset_of!(ScsiHeader, data_in)] = if length == 0 { 2 } else { 1 };
    put32(&mut buffer, offset_of!(ScsiHeader, transfer), length as u32);
    put32(&mut buffer, offset_of!(ScsiHeader, timeout), 2);
    put_size(
        &mut buffer,
        offset_of!(ScsiHeader, offset),
        if length == 0 { 0 } else { payload },
    );
    put32(
        &mut buffer,
        offset_of!(ScsiHeader, sense_offset),
        sense_at as u32,
    );
    let at = offset_of!(ScsiHeader, cdb);
    buffer[at..at + cdb.len()].copy_from_slice(cdb);
    let returned = exchange(handle, SCSI_PASS_THROUGH, &mut buffer, payload)?;
    if returned < sense_at {
        return Err(Failure::invalid());
    }
    let is_sat = matches!(cdb[0], 0x85 | 0xa1);
    let sense_len = (buffer[offset_of!(ScsiHeader, sense_length)] as usize).min(SENSE_SIZE);
    let sense = buffer
        .get(sense_at..(sense_at + sense_len).min(returned))
        .unwrap_or(&[]);
    let registers = scsi_status(buffer[offset_of!(ScsiHeader, status)], sense, is_sat)?;
    let transferred = get32(&buffer, offset_of!(ScsiHeader, transfer)) as usize;
    if transferred > length
        || (length > 0
            && (get_size(&buffer, offset_of!(ScsiHeader, offset)) != payload
                || returned < payload + transferred))
    {
        return Err(Failure::invalid());
    }
    if is_sat && transferred < length {
        return Err(Failure::invalid());
    }
    Ok(Answer {
        data: buffer[payload..payload + transferred].to_vec(),
        registers,
    })
}

fn scsi_status(status: u8, sense: &[u8], sat: bool) -> Result<Option<[u8; 8]>> {
    if status == 0 {
        return Ok(None);
    }
    if status != 2 {
        return Err(unavailable());
    }
    let response = sense.first().copied().unwrap_or(0) & 0x7f;
    let key = match response {
        0x70 | 0x71 => sense.get(2),
        0x72 | 0x73 => sense.get(1),
        _ => None,
    }
    .map(|key| key & 0x0f);
    if key == Some(5) {
        return Err(unsupported());
    }
    if !sat || !matches!(key, Some(0 | 1)) || !matches!(response, 0x72 | 0x73) {
        return Err(unavailable());
    }
    let end = 8 + *sense.get(7).ok_or_else(Failure::invalid)? as usize;
    let mut descriptors = sense.get(8..end).ok_or_else(Failure::invalid)?;
    while !descriptors.is_empty() {
        let length = *descriptors.get(1).ok_or_else(Failure::invalid)? as usize + 2;
        let descriptor = descriptors.get(..length).ok_or_else(Failure::invalid)?;
        if descriptor[0] == 9 {
            if length < 14 || descriptor[13] & 0x21 != 0 {
                return Err(unavailable());
            }
            return Ok(Some([
                descriptor[3],
                descriptor[5],
                descriptor[7],
                descriptor[9],
                descriptor[11],
                descriptor[12],
                descriptor[13],
                0,
            ]));
        }
        descriptors = &descriptors[length..];
    }
    Err(Failure::invalid())
}

fn exchange(handle: HANDLE, code: u32, buffer: &mut [u8], input_length: usize) -> Result<usize> {
    let mut returned = 0;
    unsafe {
        DeviceIoControl(
            handle,
            code,
            Some(buffer.as_ptr().cast()),
            input_length as u32,
            Some(buffer.as_mut_ptr().cast()),
            buffer.len() as u32,
            Some(&mut returned),
            None,
        )
    }
    .map_err(Failure::native)?;
    if returned as usize > buffer.len() {
        return Err(Failure::invalid());
    }
    Ok(returned as usize)
}
fn put32(data: &mut [u8], at: usize, value: u32) {
    data[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
fn get32(data: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(data[at..at + 4].try_into().unwrap())
}
fn put_size(data: &mut [u8], at: usize, value: usize) {
    data[at..at + size_of::<usize>()].copy_from_slice(&value.to_le_bytes());
}
fn get_size(data: &[u8], at: usize) -> usize {
    usize::from_le_bytes(data[at..at + size_of::<usize>()].try_into().unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transport_layouts_and_commands_are_read_only() {
        assert_eq!(size_of::<AtaHeader>(), 48);
        assert_eq!(size_of::<ScsiHeader>(), 56);
        assert_eq!(offset_of!(AtaHeader, current), 40);
        for command in [
            AtaRead::Identify,
            AtaRead::Attributes,
            AtaRead::Thresholds,
            AtaRead::Status,
        ] {
            let task = command.task_file();
            assert!(task[6] == 0xec || (task[6] == 0xb0 && [0xd0, 0xd1, 0xda].contains(&task[0])));
            for transport in [Transport::Sat12, Transport::Sat16] {
                let cdb = sat_cdb(transport, task, command.length());
                assert_eq!(cdb[2] & 0x20, 0x20);
                assert_eq!(cdb[1], if command.length() == 0 { 6 } else { 8 });
                assert_eq!(cdb[if cdb.len() == 12 { 9 } else { 14 }], task[6]);
            }
        }
    }

    #[test]
    fn sat_check_condition_needs_valid_ata_return_registers() {
        let mut sense = [0; 22];
        sense[..8].copy_from_slice(&[0x72, 1, 0, 0x1d, 0, 0, 0, 14]);
        sense[8] = 9;
        sense[9] = 12;
        sense[17] = 0x4f;
        sense[19] = 0xc2;
        sense[21] = 0x50;
        let registers = scsi_status(2, &sense, true).unwrap().unwrap();
        assert_eq!((registers[3], registers[4]), (0x4f, 0xc2));
        assert!(scsi_status(2, &sense, false).is_err());
        for len in 0..sense.len() {
            assert!(scsi_status(2, &sense[..len], true).is_err());
        }
        sense[21] |= 1;
        assert!(scsi_status(2, &sense, true).is_err());
        assert!(scsi_status(8, &[], true).is_err());
    }
}
