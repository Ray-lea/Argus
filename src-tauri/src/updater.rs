//! In-app updates from GitHub Releases.
//!
//! A background thread checks a minute after start and then daily. An update
//! is downloaded right away but installed only when the user asks, because
//! installing exits Argus. Each channel has a manifest attached to the latest
//! stable release (`latest.json`, `beta.json`, written by the release
//! workflow), fetched from GitHub directly or through a GitHub proxy. The
//! installer must carry a signature from the key in tauri.conf.json that also
//! covers its version (`requireSignedVersion`), so a proxy can withhold an
//! update but not alter or downgrade one.

use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Url};
use tauri_plugin_updater::{Error, Update, UpdaterExt};

use crate::settings::{SettingsStore, UpdateChannel, UpdateRoute};

const REPO: &str = "https://github.com/Ray-lea/Argus";
/// Public GitHub proxies that served release downloads byte for byte when
/// they were picked (2026-10). They come and go; users can enter their own.
pub const PROXY_PRESETS: [&str; 3] = [
    "https://gh-proxy.com/",
    "https://gh-proxy.org/",
    "https://ghfast.top/",
];
/// Passed to the app the installer relaunches.
pub const UPDATED_ARG: &str = "--updated";
/// Left by an update started while the window was hidden: the relaunched app
/// stays in the tray.
const RELAUNCH_HIDDEN: &str = "relaunch-hidden";

const FIRST_CHECK: Duration = Duration::from_secs(60);
const TICK: Duration = Duration::from_secs(3600);
const CHECK_EVERY_MS: i64 = 24 * 3600 * 1000;
const RETRY_AFTER_MS: i64 = 6 * 3600 * 1000;
const MANIFEST_TIMEOUT: Duration = Duration::from_secs(15);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(300);
const PROGRESS_EVERY: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    /// `disabled`, `idle`, `checking`, `upToDate`, `available` (only for the
    /// standalone exe), `downloading`, `ready`, `installing` or `error`.
    pub state: &'static str,
    pub version: Option<String>,
    /// Release notes as plain text; unlike the installer they are not signed.
    pub notes: Option<String>,
    /// Release date, Unix ms.
    pub date: Option<i64>,
    pub received: u64,
    pub total: Option<u64>,
    /// Why the last check failed, or why updates are disabled.
    pub error: Option<String>,
    /// Last finished check, Unix ms.
    pub checked_at: Option<i64>,
    /// Where the last answer came from: "GitHub" or the proxy's host.
    pub source: Option<String>,
    /// Not an installation (the standalone exe): updates are downloaded by hand.
    pub portable: bool,
}

struct Inner {
    status: UpdateStatus,
    /// The verified installer for `status.version`.
    ready: Option<(Update, Vec<u8>)>,
    next_check: i64,
}

pub struct Updates(Mutex<Inner>);

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

fn pubkey(app: &AppHandle) -> String {
    app.config()
        .plugins
        .0
        .get("updater")
        .and_then(|c| c.get("pubkey"))
        .and_then(|k| k.as_str())
        .unwrap_or_default()
        .to_string()
}

pub fn init(app: &AppHandle) {
    // The installer puts uninstall.exe beside argus.exe; the standalone exe has none.
    let portable = !std::env::current_exe()
        .map(|exe| exe.with_file_name("uninstall.exe").exists())
        .unwrap_or(false);
    let disabled = if cfg!(debug_assertions) {
        Some("开发版不检查更新。")
    } else if pubkey(app).is_empty() {
        Some("此版本没有配置更新签名公钥，无法验证更新。")
    } else {
        None
    };
    let status = UpdateStatus {
        state: if disabled.is_some() {
            "disabled"
        } else {
            "idle"
        },
        error: disabled.map(Into::into),
        portable,
        ..Default::default()
    };
    app.manage(Updates(Mutex::new(Inner {
        status,
        ready: None,
        next_check: 0,
    })));
    if disabled.is_some() {
        return;
    }
    let app = app.clone();
    let _ = std::thread::Builder::new()
        .name("argus-updates".into())
        .spawn(move || {
            std::thread::sleep(FIRST_CHECK);
            loop {
                let due = app.state::<SettingsStore>().get().update_check
                    && now_ms() >= app.state::<Updates>().0.lock().unwrap().next_check;
                if due {
                    tauri::async_runtime::block_on(check(&app));
                }
                std::thread::sleep(TICK);
            }
        });
}

pub fn status(app: &AppHandle) -> UpdateStatus {
    app.state::<Updates>().0.lock().unwrap().status.clone()
}

fn publish(app: &AppHandle, status: &UpdateStatus) {
    let _ = app.emit("update-status", status);
    let ready = status
        .version
        .as_deref()
        .filter(|_| status.state == "ready");
    crate::shell::set_tray_update(app, ready);
}

fn update_status(app: &AppHandle, f: impl FnOnce(&mut Inner)) {
    let updates = app.state::<Updates>();
    let status = {
        let mut inner = updates.0.lock().unwrap();
        f(&mut inner);
        inner.status.clone()
    };
    publish(app, &status);
}

/// Forgets a downloaded update, e.g. after switching channels.
pub fn reset(app: &AppHandle) {
    update_status(app, |inner| {
        if matches!(
            inner.status.state,
            "disabled" | "checking" | "downloading" | "installing"
        ) {
            return;
        }
        inner.ready = None;
        inner.next_check = 0;
        inner.status = UpdateStatus {
            state: "idle",
            portable: inner.status.portable,
            ..Default::default()
        };
    });
}

/// Where requests go, in order: `None` is GitHub itself, `Some` a proxy prefix.
fn sources(route: UpdateRoute, proxy: &str) -> Vec<Option<String>> {
    let mut list = Vec::new();
    if route != UpdateRoute::Proxy {
        list.push(None);
    }
    if route != UpdateRoute::Direct {
        list.push(Some(proxy.to_string()));
    }
    if route == UpdateRoute::Auto {
        for preset in PROXY_PRESETS {
            if preset != proxy {
                list.push(Some(preset.to_string()));
            }
        }
    }
    list
}

fn label(source: Option<&str>) -> String {
    source
        .and_then(|prefix| Url::parse(prefix).ok())
        .and_then(|url| url.host_str().map(str::to_string))
        .unwrap_or_else(|| "GitHub".into())
}

fn via(url: &str, source: Option<&str>) -> Result<Url, String> {
    Url::parse(&format!("{}{url}", source.unwrap_or_default())).map_err(|e| e.to_string())
}

/// Checks that a proxy prefix is an https URL the GitHub URL can follow.
pub fn normalize_proxy(proxy: &str) -> Result<String, String> {
    let proxy = proxy.trim();
    let url = Url::parse(proxy).map_err(|_| "代理地址无效".to_string())?;
    if url.scheme() != "https" || url.host_str().is_none() {
        return Err("代理地址必须以 https:// 开头".into());
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err("代理地址不能包含 ? 或 #".into());
    }
    Ok(if proxy.ends_with('/') {
        proxy.to_string()
    } else {
        format!("{proxy}/")
    })
}

fn manifest_url(channel: UpdateChannel) -> String {
    let name = match channel {
        UpdateChannel::Stable => "latest.json",
        UpdateChannel::Beta => "beta.json",
    };
    format!("{REPO}/releases/latest/download/{name}")
}

fn describe(e: &Error) -> String {
    match e {
        Error::Minisign(_) | Error::SignedVersionMismatch { .. } | Error::MissingSignedVersion => {
            "更新包签名校验失败，已丢弃".into()
        }
        Error::ReleaseNotFound => "未找到更新信息".into(),
        Error::Reqwest(e) if e.is_timeout() => "连接超时".into(),
        Error::Reqwest(e) if e.is_connect() => "无法连接".into(),
        Error::Network(message) => message.clone(),
        other => other.to_string(),
    }
}

fn signature_error(e: &Error) -> bool {
    matches!(
        e,
        Error::Minisign(_) | Error::SignedVersionMismatch { .. } | Error::MissingSignedVersion
    )
}

async fn find(
    app: &AppHandle,
    manifest: &str,
    source: Option<&str>,
) -> Result<Option<Update>, String> {
    app.updater_builder()
        .endpoints(vec![via(manifest, source)?])
        .map_err(|e| describe(&e))?
        .timeout(MANIFEST_TIMEOUT)
        // Relaunch with our own arguments: the current ones may carry a
        // one-time task manager handshake.
        .installer_args(["/R", "/ARGS", UPDATED_ARG])
        // The installer is started with std::process::exit, which skips RunEvent::Exit.
        .on_before_exit(crate::stop_network_session)
        .build()
        .map_err(|e| describe(&e))?
        .check()
        .await
        .map_err(|e| describe(&e))
}

/// Checks the chosen channel and downloads a newer release. Does nothing while
/// another check, download or install is under way.
pub async fn check(app: &AppHandle) {
    let mut busy = false;
    update_status(app, |inner| {
        busy = matches!(
            inner.status.state,
            "disabled" | "checking" | "downloading" | "installing"
        );
        if !busy {
            inner.status = UpdateStatus {
                state: "checking",
                portable: inner.status.portable,
                checked_at: inner.status.checked_at,
                ..Default::default()
            };
        }
    });
    if busy {
        return;
    }

    let settings = app.state::<SettingsStore>().get();
    let sources = sources(settings.update_route, &settings.update_proxy);
    let manifest = manifest_url(settings.update_channel);
    let mut failures = Vec::new();
    let mut found = None;
    for (index, source) in sources.iter().enumerate() {
        match find(app, &manifest, source.as_deref()).await {
            Ok(update) => {
                found = Some((index, update));
                break;
            }
            Err(e) => failures.push(format!("{}：{e}", label(source.as_deref()))),
        }
    }

    let now = now_ms();
    let Some((index, update)) = found else {
        update_status(app, |inner| {
            inner.next_check = now + RETRY_AFTER_MS;
            inner.status.state = "error";
            inner.status.error = Some(failures.join("；"));
            inner.status.checked_at = Some(now);
        });
        return;
    };
    let source = label(sources[index].as_deref());
    let Some(update) = update else {
        update_status(app, |inner| {
            inner.next_check = now + CHECK_EVERY_MS;
            inner.ready = None;
            inner.status.state = "upToDate";
            inner.status.checked_at = Some(now);
            inner.status.source = Some(source);
        });
        return;
    };

    let mut already = false;
    update_status(app, |inner| {
        inner.next_check = now + CHECK_EVERY_MS;
        already = inner
            .ready
            .as_ref()
            .is_some_and(|(ready, _)| ready.version == update.version);
        if !already {
            inner.ready = None;
        }
        inner.status = UpdateStatus {
            state: if already {
                "ready"
            } else if inner.status.portable {
                "available"
            } else {
                "downloading"
            },
            version: Some(update.version.clone()),
            notes: update.body.clone(),
            date: update.date.map(|d| d.unix_timestamp() * 1000),
            checked_at: Some(now),
            source: Some(source),
            portable: inner.status.portable,
            ..Default::default()
        };
    });
    if already || status(app).portable {
        return;
    }

    // Start with the source that answered, then try the others.
    let original = update.download_url.to_string();
    let mut order: Vec<_> = sources[index..]
        .iter()
        .chain(&sources[..index])
        .cloned()
        .collect();
    order.dedup();
    failures.clear();
    for source in order {
        let mut attempt = update.clone();
        attempt.download_url = match via(&original, source.as_deref()) {
            Ok(url) => url,
            Err(e) => {
                failures.push(e);
                continue;
            }
        };
        attempt.timeout = Some(DOWNLOAD_TIMEOUT);
        let mut received = 0u64;
        let mut last = Instant::now();
        let result = attempt
            .download(
                |chunk, total| {
                    received += chunk as u64;
                    if last.elapsed() >= PROGRESS_EVERY {
                        last = Instant::now();
                        update_status(app, |inner| {
                            inner.status.received = received;
                            inner.status.total = total;
                        });
                    }
                },
                || {},
            )
            .await;
        match result {
            Ok(bytes) => {
                let host = label(source.as_deref());
                update_status(app, |inner| {
                    inner.status.state = "ready";
                    inner.status.received = bytes.len() as u64;
                    inner.status.total = Some(bytes.len() as u64);
                    inner.status.source = Some(host);
                    inner.ready = Some((attempt.restart_after_install(false), bytes));
                });
                return;
            }
            Err(e) => {
                failures.push(format!("{}：{}", label(source.as_deref()), describe(&e)));
                // A bad artifact is bad from every source.
                if signature_error(&e) {
                    break;
                }
            }
        }
    }
    update_status(app, |inner| {
        inner.next_check = now + RETRY_AFTER_MS;
        inner.status.state = "error";
        inner.status.error = Some(format!("下载失败：{}", failures.join("；")));
    });
}

/// Starts the downloaded installer, which exits Argus and relaunches it.
/// Returns only on failure. Must not run on the main thread.
pub fn install(app: &AppHandle) -> Result<(), String> {
    let updates = app.state::<Updates>();
    let (update, bytes) = {
        let mut inner = updates.0.lock().unwrap();
        if inner.status.state != "ready" {
            return Err("没有可安装的更新".into());
        }
        let ready = inner.ready.take().ok_or("没有可安装的更新")?;
        inner.status.state = "installing";
        inner.status.error = None;
        let status = inner.status.clone();
        drop(inner);
        publish(app, &status);
        ready
    };

    let marker = app
        .path()
        .app_local_data_dir()
        .map(|dir| dir.join(RELAUNCH_HIDDEN));
    let hidden = app
        .get_webview_window("main")
        .is_some_and(|w| !w.is_visible().unwrap_or(true));
    if hidden && let Ok(marker) = &marker {
        if let Some(dir) = marker.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(marker, b"");
    }

    let error = match update.install(&bytes) {
        Ok(()) => return Ok(()),
        Err(e) => describe(&e),
    };
    if let Ok(marker) = &marker {
        let _ = std::fs::remove_file(marker);
    }
    update_status(app, |inner| {
        inner.status.state = "ready";
        inner.status.error = Some(format!("无法启动安装程序：{error}"));
        inner.ready = Some((update, bytes));
    });
    Err(error)
}

/// Whether the app was relaunched by an update started from the tray; clears the marker.
pub fn take_relaunch_hidden(app: &AppHandle) -> bool {
    app.path()
        .app_local_data_dir()
        .map(|dir| std::fs::remove_file(dir.join(RELAUNCH_HIDDEN)).is_ok())
        .unwrap_or(false)
}

/// Opens the release page of `version` (the standalone exe updates by hand).
pub fn open_release_page(version: &str) -> Result<(), String> {
    if !version
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'))
    {
        return Err("版本号无效".into());
    }
    std::process::Command::new("explorer.exe")
        .arg(format!("{REPO}/releases/tag/v{version}"))
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sources_follow_the_route() {
        let proxy = "https://example.org/";
        assert_eq!(sources(UpdateRoute::Direct, proxy), vec![None]);
        assert_eq!(
            sources(UpdateRoute::Proxy, proxy),
            vec![Some(proxy.to_string())]
        );
        let auto = sources(UpdateRoute::Auto, PROXY_PRESETS[1]);
        assert_eq!(auto[0], None);
        assert_eq!(auto[1].as_deref(), Some(PROXY_PRESETS[1]));
        assert_eq!(auto.len(), 1 + PROXY_PRESETS.len());
    }

    #[test]
    fn proxies_prefix_the_github_url() {
        let url = via(
            "https://github.com/a/b/x.json",
            Some("https://gh-proxy.com/"),
        )
        .unwrap();
        assert_eq!(
            url.as_str(),
            "https://gh-proxy.com/https://github.com/a/b/x.json"
        );
        assert_eq!(label(Some("https://gh-proxy.com/")), "gh-proxy.com");
        assert_eq!(label(None), "GitHub");
    }

    #[test]
    fn proxy_addresses_are_checked() {
        assert_eq!(
            normalize_proxy(" https://p.example ").unwrap(),
            "https://p.example/"
        );
        assert_eq!(
            normalize_proxy("https://p.example/gh/").unwrap(),
            "https://p.example/gh/"
        );
        assert!(normalize_proxy("http://p.example/").is_err());
        assert!(normalize_proxy("https://p.example/?u=").is_err());
        assert!(normalize_proxy("p.example").is_err());
    }
}
