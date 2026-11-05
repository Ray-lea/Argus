//! ATA IDENTIFY and SMART sectors. Vendor raw bytes are preserved verbatim;
//! normalized values are compared only with the matching reported threshold.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AtaIdentity {
    pub model: Option<String>,
    pub serial_number: Option<String>,
    pub firmware_revision: Option<String>,
    pub smart_supported: Option<bool>,
    pub smart_enabled: Option<bool>,
    pub rotation_rpm: Option<u16>,
    pub solid_state: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AtaAttribute {
    pub id: u8,
    pub flags: u16,
    pub current: u8,
    pub worst: u8,
    pub threshold: Option<u8>,
    /// Six bytes in device order; no vendor-specific unit conversion.
    pub raw: [u8; 6],
    pub failing: bool,
}

pub(super) fn identify(data: &[u8]) -> Option<AtaIdentity> {
    let data = sector(data)?;
    if data[510] == 0xa5 && checksum(data) != 0 {
        return None;
    }
    let word = |index: usize| u16::from_le_bytes([data[index * 2], data[index * 2 + 1]]);
    let text = |start: usize, end: usize| {
        let bytes = data[start * 2..end * 2]
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|pair| [pair[1], pair[0]])
            .collect::<Vec<_>>();
        super::nvme::text(&bytes)
    };
    let rotation = word(217);
    let model = text(27, 47);
    let serial_number = text(10, 20);
    // A successful SAT transport can still contain a zero or unrelated sector.
    if model.is_none() && serial_number.is_none() {
        return None;
    }
    Some(AtaIdentity {
        model,
        serial_number,
        firmware_revision: text(23, 27),
        smart_supported: (word(83) & 0xc000 == 0x4000).then(|| word(82) & 1 != 0),
        smart_enabled: (word(87) & 0xc000 == 0x4000).then(|| word(85) & 1 != 0),
        rotation_rpm: (0x0401..0xffff).contains(&rotation).then_some(rotation),
        solid_state: match rotation {
            1 => Some(true),
            0x0401..=0xfffe => Some(false),
            _ => None,
        },
    })
}

pub(super) fn attributes(data: &[u8]) -> Option<Vec<AtaAttribute>> {
    let data = smart_sector(data)?;
    let mut seen = [false; 256];
    let mut attributes = Vec::new();
    for entry in data[2..362].as_chunks::<12>().0 {
        let id = entry[0];
        if id == 0 {
            continue;
        }
        if std::mem::replace(&mut seen[id as usize], true) {
            return None;
        }
        attributes.push(AtaAttribute {
            id,
            flags: u16::from_le_bytes([entry[1], entry[2]]),
            current: entry[3],
            worst: entry[4],
            threshold: None,
            raw: entry[5..11].try_into().ok()?,
            failing: false,
        });
    }
    (!attributes.is_empty()).then_some(attributes)
}

pub(super) fn thresholds(data: &[u8], attributes: &mut [AtaAttribute]) -> Option<()> {
    let data = smart_sector(data)?;
    let mut entries = [None; 256];
    for entry in data[2..362].as_chunks::<12>().0 {
        if entry[0] != 0 && entries[entry[0] as usize].replace(entry[1]).is_some() {
            return None;
        }
    }
    for attribute in attributes {
        attribute.threshold = entries[attribute.id as usize];
        attribute.failing = attribute.threshold.is_some_and(|threshold| {
            threshold > 0 && attribute.current <= threshold && attribute.flags & 1 != 0
        });
    }
    Some(())
}

fn sector(data: &[u8]) -> Option<&[u8]> {
    let data = data.get(..512)?;
    (!data.iter().all(|b| *b == 0) && !data.iter().all(|b| *b == 255)).then_some(data)
}

fn checksum(data: &[u8]) -> u8 {
    data.iter().fold(0u8, |sum, byte| sum.wrapping_add(*byte))
}

fn smart_sector(data: &[u8]) -> Option<&[u8]> {
    let data = sector(data)?;
    let revision = u16::from_le_bytes([data[0], data[1]]);
    (revision != 0 && revision != u16::MAX && checksum(data) == 0).then_some(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seal(data: &mut [u8; 512]) {
        data[511] = 0;
        data[511] = 0u8.wrapping_sub(checksum(data));
    }

    #[test]
    fn smart_preserves_vendor_bytes_and_matches_thresholds_by_id() {
        let mut sector = [0; 512];
        sector[0] = 1;
        sector[2..14].copy_from_slice(&[5, 1, 0, 10, 9, 1, 2, 3, 4, 5, 6, 0]);
        sector[14..26].copy_from_slice(&[194, 2, 0, 30, 25, 35, 0, 10, 0, 50, 0, 0]);
        seal(&mut sector);
        let mut rows = attributes(&sector).unwrap();
        assert_eq!(rows[0].raw, [1, 2, 3, 4, 5, 6]);
        let mut limits = [0; 512];
        limits[0] = 1;
        limits[2..4].copy_from_slice(&[194, 40]);
        limits[14..16].copy_from_slice(&[5, 10]);
        seal(&mut limits);
        thresholds(&limits, &mut rows).unwrap();
        assert!(rows[0].failing);
        assert!(!rows[1].failing);
        assert_eq!(rows[0].threshold, Some(10));
        sector[3] ^= 1;
        assert!(attributes(&sector).is_none());
        assert!(attributes(&limits[..511]).is_none());
        assert!(attributes(&[0; 512]).is_none());
        sector[3] ^= 1;
        sector[14] = 5;
        seal(&mut sector);
        assert!(attributes(&sector).is_none());
    }

    #[test]
    fn identify_swaps_words_and_keeps_legitimate_serial_zeroes() {
        let mut data = [0; 512];
        data[20..40].copy_from_slice(b"00003241            ");
        data[54..62].copy_from_slice(b"iDks    ");
        data[164] = 1;
        data[167] = 0x40;
        data[434..436].copy_from_slice(&7200u16.to_le_bytes());
        let disk = identify(&data).unwrap();
        assert_eq!(disk.serial_number.as_deref(), Some("00002314"));
        assert_eq!(disk.model.as_deref(), Some("Disk"));
        assert_eq!(disk.rotation_rpm, Some(7200));
        assert_eq!(disk.smart_supported, Some(true));
        assert_eq!(disk.smart_enabled, None);
        data[510] = 0xa5;
        seal(&mut data);
        assert!(identify(&data).is_some());
        data[20] ^= 1;
        assert!(identify(&data).is_none());
    }
}
