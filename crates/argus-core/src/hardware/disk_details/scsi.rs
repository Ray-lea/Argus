//! Bounded SCSI INQUIRY and LOG SENSE responses. ATA sectors never enter here.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScsiIdentity {
    pub serial_number: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScsiCounter {
    pub write: bool,
    pub parameter: u16,
    pub value: u64,
}

pub(super) fn serial(data: &[u8]) -> Option<ScsiIdentity> {
    if data.get(1) != Some(&0x80) {
        return None;
    }
    let len = u16::from_be_bytes(data.get(2..4)?.try_into().ok()?) as usize;
    Some(ScsiIdentity {
        serial_number: super::nvme::text(data.get(4..4 + len)?),
    })
}

fn parameters(data: &[u8], page: u8) -> Option<Vec<(u16, &[u8])>> {
    if data.first()? & 0x7f != page || *data.get(1)? != 0 {
        return None;
    }
    let len = u16::from_be_bytes(data.get(2..4)?.try_into().ok()?) as usize;
    let mut rest = data.get(4..4 + len)?;
    let mut result = Vec::new();
    while !rest.is_empty() {
        let code = u16::from_be_bytes(rest.get(..2)?.try_into().ok()?);
        let len = *rest.get(3)? as usize;
        if result.iter().any(|(prior, _)| *prior == code) {
            return None;
        }
        result.push((code, rest.get(4..4 + len)?));
        rest = rest.get(4 + len..)?;
    }
    Some(result)
}

pub(super) fn health(data: &[u8]) -> Option<(u8, u8, Option<f32>)> {
    let parameters = parameters(data, 0x2f)?;
    let (_, value) = parameters.iter().find(|(code, _)| *code == 0)?;
    Some((
        *value.first()?,
        *value.get(1)?,
        value.get(2).and_then(|t| temperature(*t)),
    ))
}

pub(super) fn temperatures(data: &[u8]) -> Option<(Option<f32>, Option<f32>)> {
    let parameters = parameters(data, 0x0d)?;
    let reading = |code| {
        parameters
            .iter()
            .find(|(id, _)| *id == code)
            .and_then(|(_, value)| value.get(1))
            .and_then(|t| temperature(*t))
    };
    Some((reading(0), reading(1)))
}

fn temperature(value: u8) -> Option<f32> {
    (value != 255).then_some(value as f32)
}

pub(super) fn counters(data: &[u8], write: bool) -> Option<Vec<ScsiCounter>> {
    let mut result = Vec::new();
    for (parameter, value) in parameters(data, if write { 2 } else { 3 })? {
        if parameter <= 6 && !value.is_empty() && value.len() <= 8 {
            result.push(ScsiCounter {
                write,
                parameter,
                value: value
                    .iter()
                    .fold(0u64, |sum, byte| (sum << 8) | *byte as u64),
            });
        }
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scsi_pages_keep_unknown_temperatures_and_reject_truncation() {
        let health_page = [0x2f, 0, 0, 7, 0, 0, 0, 3, 0x5d, 1, 255];
        assert_eq!(health(&health_page), Some((0x5d, 1, None)));
        for len in 0..health_page.len() {
            assert!(health(&health_page[..len]).is_none());
        }
        assert!(health(&[0; 16]).is_none());
        let temp = [0x0d, 0, 0, 12, 0, 0, 0, 2, 0, 0, 0, 1, 0, 2, 0, 60];
        assert_eq!(temperatures(&temp), Some((Some(0.), Some(60.))));
        let read = [3, 0, 0, 12, 0, 6, 0, 8, 1, 0, 0, 0, 0, 0, 0, 1];
        assert_eq!(counters(&read, false).unwrap()[0].value, (1u64 << 56) | 1);
        assert!(counters(&read, true).is_none());
    }
}
