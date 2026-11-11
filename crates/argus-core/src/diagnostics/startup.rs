//! Opt-in first-call traces. Callers choose fixed phase names and numeric keys;
//! the host/probe logger makes these records durable before entering a driver.
use std::collections::HashSet;
use std::sync::{LazyLock, Mutex};
use std::time::Instant;

type Key = (&'static str, u64, u32);
static SEEN: LazyLock<Mutex<HashSet<Key>>> = LazyLock::new(Mutex::default);

fn begin(phase: &'static str, instance: u64, operation: u32) -> Option<Instant> {
    if !log::log_enabled!(target: "argus_startup", log::Level::Info) {
        return None;
    }
    if !SEEN.lock().unwrap().insert((phase, instance, operation)) {
        return None;
    }
    log::info!(target: "argus_startup", "phase={phase}; instance={instance}; operation={operation}; begin");
    Some(Instant::now())
}

/// Trace one constructor or initialization stage, only on its first call.
pub fn startup_phase<T>(phase: &'static str, call: impl FnOnce() -> T) -> T {
    startup_query(phase, 0, 0, call)
}

/// Trace the first call for a device/query pair without logging caller data.
pub fn startup_query<T>(
    phase: &'static str,
    instance: u64,
    operation: u32,
    call: impl FnOnce() -> T,
) -> T {
    let started = begin(phase, instance, operation);
    let result = call();
    if let Some(started) = started {
        log::info!(target: "argus_startup", "phase={phase}; instance={instance}; operation={operation}; end; elapsed_us={}", started.elapsed().as_micros());
    }
    result
}

/// Preserve native return values before callers turn them into optional data.
pub fn startup_native(
    phase: &'static str,
    instance: u64,
    operation: u32,
    call: impl FnOnce() -> i32,
) -> i32 {
    let started = begin(phase, instance, operation);
    let status = call();
    if let Some(started) = started {
        log::info!(target: "argus_startup", "phase={phase}; instance={instance}; operation={operation}; end; status=0x{:08X}; elapsed_us={}", status as u32, started.elapsed().as_micros());
    }
    status
}
