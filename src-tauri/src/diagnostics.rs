//! What the log records for bug reports, beyond operation failures: the
//! environment at start, which sensors this PC offers (once, after the first
//! sample), the state of the privileged features, and errors the page did not
//! handle. The log is attached to public reports, so nothing personal goes in:
//! no paths, user or machine names, command lines or addresses.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use argus_core::Snapshot;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Security::{
    GetTokenInformation, TOKEN_ELEVATION_TYPE, TOKEN_QUERY, TokenElevationType,
    TokenElevationTypeDefault, TokenElevationTypeFull, TokenElevationTypeLimited,
};
use windows::Win32::System::Registry::{
    HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegGetValueW,
};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::core::{PCWSTR, w};

/// One line about the system Argus runs on.
pub fn log_environment(elevated: bool) {
    log::info!(
        "environment: {}, WebView2 {}, {} ({}), language {}",
        windows_version(),
        tauri::webview_version().unwrap_or_else(|_| "unavailable".into()),
        if elevated {
            "administrator"
        } else {
            "standard user"
        },
        token_kind(),
        argus_core::i18n::locale().tag(),
    );
}

/// "Windows 11 24H2 (build 26100.4061)".
fn windows_version() -> String {
    const KEY: PCWSTR = w!(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion");
    let text = |name: PCWSTR| {
        let mut buf = [0u16; 64];
        let mut size = size_of_val(&buf) as u32;
        let ok = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                KEY,
                name,
                RRF_RT_REG_SZ,
                None,
                Some(buf.as_mut_ptr().cast()),
                Some(&mut size),
            )
        }
        .is_ok();
        ok.then(|| String::from_utf16_lossy(&buf[..(size as usize / 2).saturating_sub(1)]))
    };
    let mut ubr = 0u32;
    let mut size = size_of::<u32>() as u32;
    let has_ubr = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            KEY,
            w!("UBR"),
            RRF_RT_REG_DWORD,
            None,
            Some((&raw mut ubr).cast()),
            Some(&mut size),
        )
    }
    .is_ok();
    let build = text(w!("CurrentBuild")).unwrap_or_default();
    let name = if build.parse::<u32>().is_ok_and(|b| b >= 22000) {
        "Windows 11"
    } else {
        "Windows 10"
    };
    let release = text(w!("DisplayVersion"))
        .map(|v| format!(" {v}"))
        .unwrap_or_default();
    let ubr = if has_ubr {
        format!(".{ubr}")
    } else {
        String::new()
    };
    format!("{name}{release} (build {build}{ubr})")
}

/// Whether the account has a split token (UAC) and which half Argus runs with:
/// it decides how the task manager replacement starts the interface.
fn token_kind() -> &'static str {
    let mut token = HANDLE::default();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }.is_err() {
        return "token unknown";
    }
    let mut kind = TOKEN_ELEVATION_TYPE::default();
    let mut size = 0;
    let ok = unsafe {
        GetTokenInformation(
            token,
            TokenElevationType,
            Some((&raw mut kind).cast()),
            size_of::<TOKEN_ELEVATION_TYPE>() as u32,
            &mut size,
        )
    }
    .is_ok();
    unsafe {
        let _ = CloseHandle(token);
    }
    if !ok {
        "token unknown"
    } else if kind == TokenElevationTypeDefault {
        "no split token"
    } else if kind == TokenElevationTypeFull {
        "elevated split token"
    } else if kind == TokenElevationTypeLimited {
        "limited split token"
    } else {
        "token unknown"
    }
}

static HARDWARE_LOGGED: AtomicBool = AtomicBool::new(false);

/// Which sensors this PC offers, from the first sample: the usual answer to
/// "why is there no temperature / power on my machine".
pub fn log_hardware_once(s: &Snapshot) {
    if HARDWARE_LOGGED.swap(true, Ordering::Relaxed) {
        return;
    }
    let hw = &s.hardware;
    let temperature = if hw.thermal.is_empty() {
        "none (no ACPI thermal zone)".to_string()
    } else {
        format!(
            "ACPI thermal zones {}",
            hw.thermal
                .iter()
                .map(|z| z.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    let power = match (hw.power.package, hw.power.cores) {
        (Some(_), Some(_)) => "package and cores",
        (Some(_), None) => "package",
        (None, Some(_)) => "cores",
        (None, None) => "none",
    };
    log::info!(
        "cpu: {} ({} logical); temperature: {temperature}; RAPL power: {power}; clock: {}",
        hw.cpu.name,
        hw.cpu.logical,
        if hw.core_mhz.is_empty() {
            "total only"
        } else {
            "per processor"
        },
    );
    for g in &hw.gpus {
        let power = if g.board_power.is_some() {
            "watts (NVML)"
        } else if g.power.is_some() {
            "percent of budget"
        } else {
            "none"
        };
        log::info!(
            "gpu: {}; temperature: {}; power: {power}; clock: {}",
            g.info.name,
            if g.temperature.is_some() {
                "yes"
            } else {
                "none"
            },
            if g.clock_mhz.is_some() { "yes" } else { "none" },
        );
    }
    log::info!(
        "devices: {} disk(s), {} with temperature; {} network adapter(s); {} battery(ies); per-process network: {}",
        hw.disks.len(),
        hw.disks.iter().filter(|d| d.temperature.is_some()).count(),
        hw.nics.len(),
        hw.batteries.len(),
        if s.process_network { "yes" } else { "no" },
    );
}

/// The privileged service and the task manager replacement, as Argus finds
/// them at start. Both checks can take a moment, so this runs on its own thread.
pub fn log_privileged_state() {
    std::thread::spawn(|| {
        match argus_service::status() {
            Ok(s) => log::info!(
                "privileged service: {}{}{}",
                s.state,
                s.version.map(|v| format!(" {v}")).unwrap_or_default(),
                if s.authorized || s.state == "notInstalled" {
                    ""
                } else {
                    ", authorized for another account"
                },
            ),
            Err(e) => log::warn!("privileged service status unavailable: {e}"),
        }
        match argus_service::taskmgr::status() {
            Ok(s) => log::info!("task manager replacement: {}", s.state),
            Err(e) => log::warn!("task manager replacement status unavailable: {e}"),
        }
    });
}

/// Errors from the page are capped per session: a broken render loop must not
/// fill the log.
static UI_ERRORS: AtomicU32 = AtomicU32::new(0);
const UI_ERRORS_MAX: u32 = 20;

#[tauri::command]
pub fn log_ui_error(message: String) {
    let n = UI_ERRORS.fetch_add(1, Ordering::Relaxed);
    if n < UI_ERRORS_MAX {
        let text: String = scrub(&message).chars().take(2000).collect();
        log::error!("page: {text}");
    } else if n == UI_ERRORS_MAX {
        log::error!("page: more errors this session are not logged");
    }
}

/// Replaces user profile paths (`C:\Users\<name>\…`) with `<profile>`: error
/// text can carry a path, and the profile folder is named after the user.
fn scrub(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.to_ascii_lowercase().find(r"\users\") {
        let start = rest[..at]
            .rfind(|c: char| !(c.is_ascii_alphabetic() || c == ':'))
            .map_or(0, |i| i + 1);
        out.push_str(&rest[..start]);
        let after = &rest[at + r"\users\".len()..];
        let end = after
            .find(['\\', '/', '"', '\'', ' ', '\n'])
            .unwrap_or(after.len());
        out.push_str("<profile>");
        rest = &after[end..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::scrub;

    #[test]
    fn profile_paths_lose_the_user_name() {
        assert_eq!(
            scrub(r"cannot open C:\Users\someone\AppData\x.json: denied"),
            r"cannot open <profile>\AppData\x.json: denied"
        );
        assert_eq!(scrub(r"d:\USERS\a b"), r"<profile> b");
        assert_eq!(scrub("no path here"), "no path here");
    }
}
