//! What the log records for bug reports, beyond operation failures: the
//! environment at start, which sensors this PC offers (once, after the first
//! sample), the state of the privileged features, and errors the page did not
//! handle. Reviewed failure paths are explicit fields; raw errors, command
//! arguments, account names, machine names and network addresses are excluded.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::{fs::File, io::Read};

use argus_core::{Snapshot, hardware::GpuPowerSource};
use sha2::{Digest, Sha256};
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
        tauri::webview_version()
            .ok()
            .filter(|v| v.len() <= 32 && v.bytes().all(|c| c.is_ascii_digit() || c == b'.'))
            .unwrap_or_else(|| "unavailable".into()),
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
    let build = text(w!("CurrentBuild"))
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or_default();
    let name = if build >= 22000 {
        "Windows 11"
    } else {
        "Windows 10"
    };
    let release = text(w!("DisplayVersion"))
        .filter(|v| v.len() <= 16 && v.bytes().all(|c| c.is_ascii_alphanumeric()))
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
/// Samples so far without per-process network; `u32::MAX` once it was there.
static WITHOUT_NETWORK: AtomicU32 = AtomicU32::new(0);

/// Reviewed model fields only, not arbitrary error text or device instance IDs.
/// Registry descriptions can contain control characters, so keep each label
/// bounded and escaped before including it in the shareable startup log.
fn hardware_label(value: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        return "unavailable".into();
    }
    value
        .chars()
        .take(128)
        .flat_map(char::escape_default)
        .collect()
}

/// Which sensors this PC offers, from the first sample: the usual answer to
/// "why is there no temperature / power on my machine". Runs for every sample:
/// per-process network comes from the privileged service or the ETW session,
/// which answer a sample or more after the first, so its line follows when it
/// turns up.
pub fn log_hardware_once(s: &Snapshot) {
    let (without, from) = network_step(WITHOUT_NETWORK.load(Ordering::Relaxed), s.process_network);
    // Only the sampling thread calls this.
    WITHOUT_NETWORK.store(without, Ordering::Relaxed);
    if let Some(sample) = from {
        log::info!("per-process network: yes from sample {sample}");
    }
    if HARDWARE_LOGGED.swap(true, Ordering::Relaxed) {
        return;
    }
    let hw = &s.hardware;
    let temperature = if hw.thermal.is_empty() {
        "none (no ACPI thermal zone)".to_string()
    } else {
        format!("ACPI thermal zones {}", hw.thermal.len())
    };
    let power = match (hw.power.package, hw.power.cores) {
        (Some(_), Some(_)) => "package and cores",
        (Some(_), None) => "package",
        (None, Some(_)) => "cores",
        (None, None) => "none",
    };
    log::info!(
        "cpu: model=\"{}\"; family={}; model_id={}; stepping={}; {} logical; temperature: {temperature}; RAPL power: {power}; clock: {}",
        hardware_label(&hw.cpu.name),
        hw.cpu.family,
        hw.cpu.model,
        hw.cpu.stepping,
        hw.cpu.logical,
        if hw.core_mhz.is_empty() {
            "total only"
        } else {
            "per processor"
        },
    );
    for (index, g) in hw.gpus.iter().enumerate() {
        let power = if g.board_power.is_some() {
            match g.power_source {
                Some(GpuPowerSource::Nvml) => "watts (NVML board)",
                Some(GpuPowerSource::AdlBoard) => "watts (ADL board)",
                Some(GpuPowerSource::AdlChip) => "watts (ADL chip)",
                Some(GpuPowerSource::Igcl) => "watts (IGCL GPU)",
                Some(GpuPowerSource::IgclChip) => "watts (IGCL chip)",
                None => "watts (source unavailable)",
            }
        } else if g.power.is_some() {
            "percent of budget"
        } else {
            "none"
        };
        log::info!(
            "gpu: {}; model=\"{}\"; temperature: {}; power: {power}; clock: {}",
            index,
            hardware_label(&g.info.name),
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
        if s.process_network {
            "yes"
        } else {
            "not in the first sample"
        },
    );
}

/// The count of samples without per-process network after one more, and the
/// sample (from 1) to report it from when it turned up after the first.
fn network_step(without: u32, available: bool) -> (u32, Option<u32>) {
    match (without, available) {
        (u32::MAX, _) => (u32::MAX, None),
        (0, true) => (u32::MAX, None),
        (n, true) => (u32::MAX, Some(n.saturating_add(1))),
        (n, false) => (n.saturating_add(1).min(u32::MAX - 1), None),
    }
}

fn executable_fingerprint() -> std::io::Result<String> {
    let mut image = File::open(std::env::current_exe()?)?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let bytes = image.read(&mut buffer)?;
        if bytes == 0 {
            return Ok(hash
                .finalize()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect());
        }
        hash.update(&buffer[..bytes]);
    }
}

/// The privileged service and the task manager replacement, as Argus finds
/// them at start. Both checks can take a moment, so this runs on its own thread.
pub fn log_privileged_state() {
    std::thread::spawn(|| {
        // Test packages can share a release number. Hash the running image,
        // including a task-manager copy, off the UI and sampling threads.
        match executable_fingerprint() {
            Ok(hash) => log::info!("running binary: sha256={hash}"),
            Err(error) => log::warn!(
                "running binary fingerprint unavailable: os error {:?}",
                error.raw_os_error()
            ),
        }
        match argus_service::status() {
            Ok(s) => log::info!(
                "privileged service: {}{}{}{}",
                s.state,
                match &s.version {
                    Some(v) if v == env!("CARGO_PKG_VERSION") => " (same release version)",
                    Some(_) => " (other version)",
                    None => "",
                },
                if s.authorized || s.state == "notInstalled" {
                    ""
                } else {
                    ", authorized for another account"
                },
                argus_service::status_issue(&s)
                    .map(|issue| format!(", error: {issue}"))
                    .unwrap_or_default(),
            ),
            Err(e) => log::warn!(
                "privileged service status unavailable: {}",
                argus_core::diagnostics::failure(&e)
            ),
        }
        match argus_service::taskmgr::status() {
            Ok(s) => log::info!("task manager replacement: {}", s.summary()),
            Err(e) => log::warn!(
                "task manager replacement status unavailable: {}",
                argus_core::diagnostics::failure(&e)
            ),
        }
    });
}

#[derive(serde::Deserialize, Debug)]
pub struct ErrorFrame {
    file: String,
    line: u32,
    column: u32,
}

fn frame_location(frame: &ErrorFrame) -> Option<String> {
    let file = &frame.file;
    let script =
        (file.starts_with("assets/") && file.matches('/').count() == 1 && file.ends_with(".js"))
            || (file.starts_with("src/") && (file.ends_with(".ts") || file.ends_with(".tsx")));
    (script
        && file.len() <= 180
        && file
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/_-.".contains(&b))
        && file.split('/').all(|part| !matches!(part, "" | "." | ".."))
        && (1..=1_000_000).contains(&frame.line)
        && frame.column <= 10_000_000)
        .then(|| format!("{}:{}:{}", file, frame.line, frame.column))
}

#[tauri::command]
pub fn log_ui_error(
    event: String,
    category: String,
    frames: Option<Vec<ErrorFrame>>,
    native_code: Option<u32>,
    action_kind: Option<String>,
    value_type: Option<String>,
    command: Option<String>,
) {
    let event = match event.as_str() {
        "ipc" => "page IPC failure",
        "unhandledRejection" => "page unhandled rejection",
        _ => "page uncaught exception",
    };
    let frames: Vec<_> = frames
        .unwrap_or_default()
        .iter()
        .take(5)
        .filter_map(frame_location)
        .collect();
    let action = action_kind
        .as_deref()
        .filter(|kind| {
            matches!(
                *kind,
                "accessDenied" | "notFound" | "critical" | "refused" | "serviceOutdated" | "os"
            )
        })
        .unwrap_or("none");
    let value_type = value_type
        .as_deref()
        .filter(|value| {
            matches!(
                *value,
                "null"
                    | "undefined"
                    | "object"
                    | "string"
                    | "number"
                    | "boolean"
                    | "bigint"
                    | "symbol"
                    | "function"
            )
        })
        .unwrap_or("unknown");
    let command = command
        .as_deref()
        .filter(|command| {
            !command.is_empty()
                && command.len() <= 64
                && command.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
        })
        .unwrap_or("none");
    argus_core::diagnostics::warn(
        event,
        format!(
            "category={}; value_type={value_type}; action_kind={action}; command={command}; native={}; frames={frames:?}",
            argus_core::diagnostics::exception(&category),
            native_code
                .map(argus_core::diagnostics::code)
                .unwrap_or_else(|| "none".into())
        ),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hardware_labels_are_bounded_and_cannot_forge_log_lines() {
        assert_eq!(hardware_label("  Example CPU  "), "Example CPU");
        assert_eq!(hardware_label(&"x".repeat(129)).len(), 128);
        let label = hardware_label("GPU\n\r\t\"\\\u{202e}");
        assert!(!label.chars().any(char::is_control));
        assert!(label.contains("\\n"));
        assert!(label.contains("\\\""));
        assert!(label.contains("\\u{202e}"));
        assert!(!hardware_label(" \t ").is_empty());
    }

    #[test]
    fn frontend_frames_are_validated_again_at_the_native_boundary() {
        for file in ["assets/index-a1.js", "src/lib/ipc.ts", "src/main.tsx"] {
            assert!(
                frame_location(&ErrorFrame {
                    file: file.into(),
                    line: 42,
                    column: 3
                })
                .is_some()
            );
        }
        for file in [
            "C:/Users/Private/a.js",
            "src/../private.ts",
            "src//bad.ts",
            "assets/a.js?secret",
            "assets/a.js\nforged",
            "https://tauri.localhost/assets/a.js",
        ] {
            assert!(
                frame_location(&ErrorFrame {
                    file: file.into(),
                    line: 42,
                    column: 3
                })
                .is_none()
            );
        }
    }

    #[test]
    fn per_process_network_is_reported_when_it_turns_up() {
        // There in the first sample: the devices line says so, nothing more.
        assert_eq!(network_step(0, true), (u32::MAX, None));
        // Missing at first, there from the third sample: one line for it.
        let (n, from) = network_step(0, false);
        assert_eq!(from, None);
        let (n, from) = network_step(n, false);
        assert_eq!(from, None);
        let (n, from) = network_step(n, true);
        assert_eq!(from, Some(3));
        // Once only, whatever follows.
        assert_eq!(network_step(n, false), (u32::MAX, None));
        assert_eq!(network_step(n, true), (u32::MAX, None));
        // Never there: the count stops short of the "was there" mark.
        assert_eq!(network_step(u32::MAX - 1, false), (u32::MAX - 1, None));
    }
}
