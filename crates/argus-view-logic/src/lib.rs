//! The interface's logic that needs no GPUI: process rows and their grouping,
//! the process columns, display filtering, the preference store, regional
//! formats, sort keys, the history page's series and the taskbar monitor's
//! layout.
//!
//! It lives outside the interface crate (`argus`, `src-gpui`) so that a
//! change here, and its tests, compile in seconds instead of rebuilding the
//! whole interface. The interface keeps each module at its old path as a
//! re-export (`src-gpui/src/frontend/model.rs` and so on), so its callers
//! name the items as before. Nothing here may depend on GPUI; what needs it
//! stays in the interface.

pub mod catalog;
pub mod collation;
pub mod columns;
pub mod gpu_usage;
pub mod history_series;
pub mod model;
pub mod prefs;
pub mod regional;
pub mod taskbar_layout;
pub mod visibility;
