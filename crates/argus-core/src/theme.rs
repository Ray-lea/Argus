//! System personalization colors.

use serde::Serialize;
use windows::UI::Color;
use windows::UI::ViewManagement::{UIColorType, UISettings};

/// The user's accent color and its system-generated shades, as `#rrggbb`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccentColors {
    pub accent: String,
    pub light1: String,
    pub light2: String,
    pub light3: String,
    pub dark1: String,
    pub dark2: String,
    pub dark3: String,
}

pub fn accent_colors() -> Option<AccentColors> {
    let settings = UISettings::new().ok()?;
    let get = |t: UIColorType| settings.GetColorValue(t).ok().map(hex);
    Some(AccentColors {
        accent: get(UIColorType::Accent)?,
        light1: get(UIColorType::AccentLight1)?,
        light2: get(UIColorType::AccentLight2)?,
        light3: get(UIColorType::AccentLight3)?,
        dark1: get(UIColorType::AccentDark1)?,
        dark2: get(UIColorType::AccentDark2)?,
        dark3: get(UIColorType::AccentDark3)?,
    })
}

fn hex(c: Color) -> String {
    format!("#{:02x}{:02x}{:02x}", c.R, c.G, c.B)
}
