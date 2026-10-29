//! Host integration for the per-account administrator startup grant
//! (`argus_service::admin`).
use crate::native_runtime::Host;
use std::sync::{Mutex, OnceLock};

static START_ARGS: OnceLock<Vec<String>> = OnceLock::new();
static DEFERRED: Mutex<Option<bool>> = Mutex::new(None);
static FAILURE: OnceLock<String> = OnceLock::new();

/// Before single-instance ownership and before any window: relaunches through
/// the grant when it is enabled. Returns whether this process should end.
pub fn route(id: &str, args: &[String]) -> bool {
    let _ = START_ARGS.set(args.to_vec());
    if !argus_service::admin::supported() {
        return false;
    }
    if args.iter().any(|a| a == argus_service::admin::FALLBACK) {
        let _ = FAILURE.set(argus_core::tr!("admin.fallback"));
        return false;
    }
    if args
        .iter()
        .any(|a| a == argus_service::admin::STANDARD_ONCE)
    {
        return false;
    }
    if argus_core::privilege::is_elevated() || !argus_service::admin::enabled(id) {
        return false;
    }
    match argus_service::admin::prepare(id, args, true).and_then(|p| p.commit()) {
        Ok(()) => true,
        Err(error) => {
            let _ = FAILURE.set(error);
            false
        }
    }
}

/// The interface shows its first data: the start that got here succeeded.
pub fn painted() {
    if let Some(args) = START_ARGS.get() {
        argus_service::admin::acknowledge(args);
    }
}

pub fn status(app: &Host) -> argus_service::admin::Status {
    let mut status = argus_service::admin::status(&app.config().identifier);
    if status.enabled && status.error.is_none() {
        status.error = FAILURE.get().cloned();
    }
    status
}

/// Turns the administrator start on or off. Unless `defer`red to the next
/// `apply_mode`, the switch restarts Argus at once with the new privileges.
pub fn set_mode(
    app: &Host,
    enabled: bool,
    defer: bool,
    repair: Option<bool>,
) -> Result<argus_service::admin::Status, String> {
    let id = &app.config().identifier;
    let old_source = argus_service::admin::source(id);
    let restart = enabled || argus_service::admin::can_drop_elevation()?;
    let pending = if !enabled && restart && !defer {
        Some(argus_service::admin::prepare(
            id,
            &restart_args(app),
            false,
        )?)
    } else {
        None
    };
    let current = argus_service::admin::status(id);
    let status = if enabled && current.ready && !repair.unwrap_or(false) {
        current
    } else {
        argus_service::admin::manage(id, enabled)?
    };
    if status.enabled != enabled || (enabled && !status.ready) {
        return Err(status
            .error
            .unwrap_or_else(|| argus_core::tr!("admin.notReady")));
    }
    // Keep the existing Startup Apps toggle; only its target follows the policy.
    crate::commands::autostart::follow_admin(app, old_source.as_deref());
    if defer {
        *DEFERRED.lock().unwrap() = Some(enabled);
    } else if restart {
        let prepared = match pending {
            Some(prepared) => prepared,
            None => argus_service::admin::prepare(id, &restart_args(app), enabled)?,
        };
        prepared.commit()?;
        app.exit(0);
    }
    Ok(status)
}

/// A restart keeps a hidden window hidden.
fn restart_args(app: &Host) -> Vec<String> {
    let hidden = app
        .get_window("main")
        .is_some_and(|w| !w.is_visible().unwrap_or(false));
    if hidden {
        vec!["--hidden".into()]
    } else {
        Vec::new()
    }
}

/// Applies a change `set_mode` deferred (onboarding finishes first).
pub fn apply_mode(app: &Host) -> Result<(), String> {
    let desired = DEFERRED.lock().unwrap().take();
    if let Some(enabled) = desired
        && (enabled || argus_service::admin::can_drop_elevation()?)
    {
        argus_service::admin::prepare(&app.config().identifier, &restart_args(app), enabled)?
            .commit()?;
        app.exit(0);
    }
    Ok(())
}
