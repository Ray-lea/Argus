//! Availability belongs to each metric, independently of its numeric value.
use serde::Serialize;

pub const UNAVAILABLE: u8 = 1;
pub const ACCESS_DENIED: u8 = 2;
pub const STALE: u8 = 4;
pub const BASELINE: u8 = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Measurement {
    pub valid: bool,
    pub partial: bool,
    /// A union of observed missing reasons, including missing parts of a sum.
    pub missing: u8,
}

impl Default for Measurement {
    fn default() -> Self {
        Self::missing(UNAVAILABLE)
    }
}

impl Measurement {
    pub const fn ready() -> Self {
        Self {
            valid: true,
            partial: false,
            missing: 0,
        }
    }
    pub const fn missing(reason: u8) -> Self {
        Self {
            valid: false,
            partial: false,
            missing: reason,
        }
    }
    pub fn failure(code: u32) -> Self {
        Self::missing(if matches!(code, 5 | 0x80070005 | 0xc0000022) {
            ACCESS_DENIED
        } else {
            UNAVAILABLE
        })
    }
    pub fn combine(values: impl IntoIterator<Item = Self>) -> Self {
        let mut result = Self {
            valid: false,
            partial: false,
            missing: 0,
        };
        let mut missing = false;
        for value in values {
            result.valid |= value.valid;
            result.partial |= value.partial;
            result.missing |= value.missing;
            missing |= !value.valid;
        }
        result.partial |= result.valid && missing;
        if !result.valid && result.missing == 0 {
            result.missing = UNAVAILABLE;
        }
        result
    }
}
