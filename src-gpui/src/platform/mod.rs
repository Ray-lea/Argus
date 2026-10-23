//! Platform adapters own native window, activation and shell integration.
#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub(crate) use windows::*;
