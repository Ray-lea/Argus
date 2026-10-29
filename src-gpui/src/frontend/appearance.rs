//! Interface scale and base font size (the original's `lib/appearance.ts`).
//!
//! The scale zooms the whole window as page zoom did in the original WebView:
//! the window's scale factor grows and its viewport shrinks, so layout,
//! painting, pointer positions and the native caption hit testing agree. The
//! font size changes text only (`theme::font`), as the original's
//! `calc(N rem / 13)` sizes did; spacing and controls keep their pixels.
use super::{prefs::Prefs, theme};
use gpui_kit::{App, Window};
use serde_json::Value;
use std::ops::RangeInclusive;

/// `uiScale`: percent of the display scale.
pub static SCALE: Setting = Setting {
    key: "uiScale",
    range: 80..=150,
    default: 100,
};
/// `fontSize`: the base font size in pixels.
pub static FONT_SIZE: Setting = Setting {
    key: "fontSize",
    range: 11..=20,
    default: 13,
};

pub struct Setting {
    pub key: &'static str,
    pub range: RangeInclusive<u32>,
    pub default: u32,
}

impl Setting {
    /// The stored value when it is an integer in range (as the original
    /// validates it), else the default.
    pub fn load(&self, prefs: &Prefs) -> u32 {
        self.valid(&prefs.get(self.key, Value::Null))
            .unwrap_or(self.default)
    }

    fn valid(&self, value: &Value) -> Option<u32> {
        let value = value.as_f64()?;
        (value.fract() == 0.
            && value >= f64::from(*self.range.start())
            && value <= f64::from(*self.range.end()))
        .then_some(value as u32)
    }

    /// What typed text commits as: a number is rounded into the range,
    /// anything else gives the default (the original's `onChange`).
    pub fn parse(&self, text: &str) -> u32 {
        match text.trim().parse::<f64>() {
            Ok(value) if value.is_finite() => value
                .clamp(f64::from(*self.range.start()), f64::from(*self.range.end()))
                .round() as u32,
            _ => self.default,
        }
    }

    /// One step of the box's text up or down, as Chromium steps a number
    /// input (`step="1"`) from its spin button or arrow keys: text that is not
    /// a number starts at the minimum, a value past the end it moves away
    /// from comes back to that end, a fraction goes to the next whole number
    /// that way, and a value at (or past) the end it moves toward stays
    /// (`None`).
    pub fn spin(&self, text: &str, up: bool) -> Option<u32> {
        let (min, max) = (*self.range.start(), *self.range.end());
        let Some(value) = text.trim().parse::<f64>().ok().filter(|v| v.is_finite()) else {
            return Some(min);
        };
        let (low, high) = (f64::from(min), f64::from(max));
        let next = if up {
            if value < low {
                return Some(min);
            }
            if value >= high {
                return None;
            }
            if value.fract() == 0. {
                value + 1.
            } else {
                value.ceil()
            }
        } else {
            if value > high {
                return Some(max);
            }
            if value <= low {
                return None;
            }
            if value.fract() == 0. {
                value - 1.
            } else {
                value.floor()
            }
        };
        Some(next.clamp(low, high) as u32)
    }
}

/// Zooms the window's content to `scale` percent of the display scale.
pub fn apply_scale(scale: u32, window: &mut Window, cx: &mut App) {
    window.set_content_zoom(scale as f32 / 100., cx);
}

/// Sets the base font size text sizes follow. The caller applies the theme
/// again and redraws the window.
pub fn apply_font_size(size: u32) {
    theme::set_font_size(size as f32);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn stored_values_are_integers_in_range() {
        assert_eq!(SCALE.valid(&json!(125)), Some(125));
        assert_eq!(SCALE.valid(&json!(125.0)), Some(125));
        assert_eq!(SCALE.valid(&json!(125.5)), None);
        assert_eq!(SCALE.valid(&json!(999)), None);
        assert_eq!(SCALE.valid(&json!("125")), None);
        assert_eq!(FONT_SIZE.valid(&json!(11)), Some(11));
        assert_eq!(FONT_SIZE.valid(&json!(21)), None);
    }

    #[test]
    fn typed_values_round_into_range() {
        assert_eq!(SCALE.parse("125"), 125);
        assert_eq!(SCALE.parse(" 124.6 "), 125);
        assert_eq!(SCALE.parse("10"), 80);
        assert_eq!(SCALE.parse("1e3"), 150);
        assert_eq!(SCALE.parse(""), 100);
        assert_eq!(SCALE.parse("abc"), 100);
        assert_eq!(FONT_SIZE.parse("19.5"), 20);
        assert_eq!(FONT_SIZE.parse("-4"), 11);
    }

    /// The cases measured on the original's font size box (11–20).
    #[test]
    fn spinning_steps_text_like_chromium() {
        let spin = |text, up| FONT_SIZE.spin(text, up);
        assert_eq!(spin("15", true), Some(16));
        assert_eq!(spin("15", false), Some(14));
        assert_eq!(spin("20", true), None);
        assert_eq!(spin("11", false), None);
        assert_eq!(spin("", true), Some(11));
        assert_eq!(spin("", false), Some(11));
        assert_eq!(spin("25", false), Some(20));
        assert_eq!(spin("25", true), None);
        assert_eq!(spin("5", true), Some(11));
        assert_eq!(spin("5", false), None);
        assert_eq!(spin("-3", true), Some(11));
        assert_eq!(spin("1e1", true), Some(11));
        assert_eq!(spin("12.5", true), Some(13));
        assert_eq!(spin("12.2", true), Some(13));
        assert_eq!(spin("12.5", false), Some(12));
        assert_eq!(spin("12.2", false), Some(12));
        assert_eq!(SCALE.spin("150", true), None);
        assert_eq!(SCALE.spin("abc", false), Some(80));
    }
}
