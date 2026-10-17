//! Argus core: system data collection and process control for Windows.
//!
//! This crate is deliberately UI-agnostic. Everything here can later move into
//! a privileged service process and be driven over IPC without changes to the
//! data model.

#![cfg(windows)]

pub mod actions;
pub mod app_windows;
pub mod collector;
pub mod connections;
pub mod details;
pub mod diagnostics;
pub mod disk_process;
pub mod filelock;
pub mod gpu_process;
pub mod handles;
pub mod hardware;
pub mod history;
pub mod i18n;
pub mod meta;
pub mod net_process;
mod nt;
mod paths;
mod pdh;
pub mod privilege;
pub mod process;
pub mod rules;
pub mod services;
pub mod startup;
pub mod system;
pub mod theme;
mod util;

pub use collector::{Collector, Snapshot};
pub use meta::MetaStore;
pub use process::ProcessRow;
pub use system::SystemStats;
