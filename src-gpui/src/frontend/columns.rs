//! Process column catalog and validated preferences
//! (`argus_view_logic::columns`, no GPUI), and the selection dialog.

use super::*;
// The whole catalog at its old path, whether or not the interface names
// each item (the process rows use the rest).
#[allow(unused_imports)]
pub(super) use argus_view_logic::columns::{
    CATEGORIES, DEFAULTS, DEFINITIONS, Definition, Format, PREFERENCE, Preferences, Sources,
    byte_count, definition, disk, disk_requests, matching, power_label, power_level, rank,
    rank_levels, raw_key, shown,
};

mod dialog;
