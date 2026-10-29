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

    /// One step up or down from `value`, kept in range.
    pub fn step(&self, value: u32, up: bool) -> u32 {
        let next = if up {
            value.saturating_add(1)
        } else {
            value.saturating_sub(1)
        };
        next.clamp(*self.range.start(), *self.range.end())
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
        assert_eq!(FONT_SIZE.step(20, true), 20);
        assert_eq!(FONT_SIZE.step(13, false), 12);
    }
}
