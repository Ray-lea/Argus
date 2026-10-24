//! Exercise the exact vendored cache with the same GPUI style types as the app.
pub use gpui_kit::gpui::{FontWeight, Refineable, TextStyle, TextStyleRefinement, px};

#[path = "../../vendor/gpui-pre/src/text_style_stack.rs"]
mod text_style_stack;
