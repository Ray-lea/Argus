//! In-app updates from GitHub Releases.
//!
//! A background thread checks a minute after start and then daily. An update
//! is downloaded right away but installed only when the user asks, because
//! installing exits Argus. Each channel has a manifest attached to the latest
//! stable release (`latest.json`, `beta.json`, written by the release
//! workflow), fetched from GitHub directly or through a GitHub proxy. The
//! installer must carry a signature from the key in tauri.conf.json that also
//! covers its version (`requireSignedVersion`), so a proxy can withhold an
//! update but not alter or downgrade one. The release notes the update dialog
//! shows are signed the same way; notes that fail the check are not shown.
//!
//! A downloaded installer is kept in the app's local data folder together
//! with what the dialog shows about it, so it survives a restart. It is
//! checked again when Argus starts and right before it runs, and removed once
//! its version is installed. Argus starts it itself, as the plugin would: the
//! plugin can only install an update it checked during this run.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use argus_core::tr;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, Url};
use tauri_plugin_updater::{Error, Update, UpdaterExt};

use crate::settings::{SettingsStore, UpdateChannel, UpdateRoute};

/// The project's page on GitHub: releases live under it, and Settings > About links to it.
pub(crate) const REPO: &str = "https://github.com/Ray-lea/Argus";
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
/// The kept installer and what is known about it, in the app's local data folder.
const UPDATES_DIR: &str = "updates";
const PENDING: &str = "pending.json";

const FIRST_CHECK: Duration = Duration::from_secs(60);
const TICK: Duration = Duration::from_secs(3600);
const CHECK_EVERY_MS: i64 = 24 * 3600 * 1000;
const RETRY_AFTER_MS: i64 = 6 * 3600 * 1000;
const MANIFEST_TIMEOUT: Duration = Duration::from_secs(15);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(300);
const PROGRESS_EVERY: Duration = Duration::from_millis(250);
/// Bounds on the release notes a manifest may carry.
const MAX_NOTES_BYTES: usize = 512 * 1024;
const MAX_RELEASES: usize = 50;

/// One changelog entry, as CHANGELOG.md has it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReleaseEntry {
    pub version: String,
    /// `YYYY-MM-DD`.
    pub date: String,
    /// The entry's Markdown.
    pub notes: String,
    /// The entry in other languages, by locale (`en-US`: CHANGELOG.en.md).
    /// Notes signed before 0.3 have none.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub translations: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    /// `disabled`, `idle`, `checking`, `upToDate`, `available` (only for the
    /// standalone exe), `downloading`, `ready`, `installing` or `error`.
    pub state: &'static str,
    pub version: Option<String>,
    /// Signed changelog entries of `version` and the releases before it,
    /// newest first.
    pub releases: Option<Vec<ReleaseEntry>>,
    /// Plain-text notes of a manifest without signed ones (releases before
    /// 0.2). Not signed; not set when signed notes failed the check.
    pub notes: Option<String>,
    /// Release date, Unix ms.
    pub date: Option<i64>,
    pub received: u64,
    /// The installer's size once known.
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

/// Release notes as a manifest carries them (`argus` in latest.json): the
/// exact text that was signed, and its signature.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SignedNotes {
    release_notes: String,
    signature: String,
}

#[derive(Deserialize)]
struct NotesPayload {
    version: String,
    releases: Vec<ReleaseEntry>,
}

/// A downloaded installer kept for later, saved as `pending.json`.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Pending {
    version: String,
    channel: UpdateChannel,
    /// The installer's file name in the updates folder.
    file: String,
    signature: String,
    date: Option<i64>,
    notes: Option<String>,
    signed_notes: Option<SignedNotes>,
    source: Option<String>,
}

/// The verified installer of `version`, on disk.
#[derive(Clone)]
struct Ready {
    version: String,
    path: PathBuf,
    signature: String,
}

struct Inner {
    status: UpdateStatus,
    ready: Option<Ready>,
    next_check: i64,
    /// A check is running (`check`); there is never more than one.
    checking: bool,
    /// Bumped by `reset`: what a running check finds after that belongs to
    /// the channel before, so it drops it and looks again.
    generation: u64,
}

impl Inner {
    fn new(status: UpdateStatus) -> Self {
        Self {
            status,
            ready: None,
            next_check: 0,
            checking: false,
            generation: 0,
        }
    }

    /// Forgets the downloaded update (another channel), unless an installer
    /// is starting. A check under way looks again once it ends, so meanwhile
    /// the state says checking. Returns whether it forgot.
    fn reset(&mut self) -> bool {
        if matches!(self.status.state, "disabled" | "installing") {
            return false;
        }
        self.generation += 1;
        self.ready = None;
        self.next_check = 0;
        self.status = UpdateStatus {
            state: if self.checking { "checking" } else { "idle" },
            portable: self.status.portable,
            ..Default::default()
        };
        true
    }

    /// Starts a check unless one is running: its generation.
    fn begin_check(&mut self) -> Option<u64> {
        if self.checking {
            return None;
        }
        self.checking = true;
        Some(self.generation)
    }

    /// Ends a check of `generation`, or returns the generation to check again
    /// for when a reset came meanwhile. Under the same lock as `reset`, so a
    /// reset either reaches this check or finds none running and the caller
    /// starts one.
    fn end_check(&mut self, generation: u64) -> Option<u64> {
        if self.generation != generation {
            return Some(self.generation);
        }
        self.checking = false;
        None
    }

    /// Whether what a check of `generation` found still applies: not after a
    /// reset, and never while an installer is starting, whose state only the
    /// install itself ends.
    fn current(&self, generation: u64) -> bool {
        self.generation == generation && self.status.state != "installing"
    }

    /// Hands out the ready installer to run, now installing.
    fn start_install(&mut self) -> Option<Ready> {
        let ready = self
            .ready
            .clone()
            .filter(|_| self.status.state == "ready")?;
        self.status.state = "installing";
        self.status.error = None;
        Some(ready)
    }

    /// The installer did not start: offered again, with `error` unless the
    /// administrator prompt was declined; or, `changed` on disk, dropped.
    fn install_failed(&mut self, changed: bool, error: Option<String>) {
        if self.status.state != "installing" {
            return;
        }
        if changed {
            self.ready = None;
            self.next_check = 0;
            self.status.state = "error";
        } else {
            self.status.state = "ready";
        }
        self.status.error = error;
    }
}

pub struct Updates(Mutex<Inner>);

/// Why an update was not installed. `cancelled`: the administrator prompt
/// was declined, which is no failure.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallError {
    cancelled: bool,
    message: String,
}

impl InstallError {
    fn failed(message: String) -> Self {
        Self {
            cancelled: false,
            message,
        }
    }
}

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
        Some(tr!("updater.devBuild"))
    } else if pubkey(app).is_empty() {
        Some(tr!("updater.noPublicKey"))
    } else {
        None
    };
    let status = UpdateStatus {
        state: if disabled.is_some() {
            "disabled"
        } else {
            "idle"
        },
        error: disabled.clone(),
        portable,
        ..Default::default()
    };
    app.manage(Updates(Mutex::new(Inner::new(status))));
    if disabled.is_some() {
        return;
    }
    let app = app.clone();
    let _ = std::thread::Builder::new()
        .name("argus-updates".into())
        .spawn(move || {
            if !portable {
                restore(&app);
            }
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
    let offered = status
        .version
        .as_deref()
        .filter(|_| matches!(status.state, "ready" | "available"));
    crate::shell::set_tray_update(app, offered);
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

/// Applies what a check of `generation` found, unless that no longer applies
/// (`Inner::current`). Returns whether it did.
fn update_found(app: &AppHandle, generation: u64, f: impl FnOnce(&mut Inner)) -> bool {
    let updates = app.state::<Updates>();
    let status = {
        let mut inner = updates.0.lock().unwrap();
        if !inner.current(generation) {
            return false;
        }
        f(&mut inner);
        inner.status.clone()
    };
    publish(app, &status);
    true
}

/// Forgets a downloaded update, e.g. after switching channels; a check under
/// way drops what it finds and looks again.
pub fn reset(app: &AppHandle) {
    let mut forgot = false;
    update_status(app, |inner| forgot = inner.reset());
    if forgot {
        discard(app);
    }
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
    let url = Url::parse(proxy).map_err(|_| tr!("updater.proxyInvalid"))?;
    if url.scheme() != "https" || url.host_str().is_none() {
        return Err(tr!("updater.proxyNotHttps"));
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(tr!("updater.proxyQuery"));
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
            tr!("updater.signatureFailed")
        }
        Error::ReleaseNotFound => tr!("updater.notFound"),
        Error::Reqwest(e) if e.is_timeout() => tr!("updater.timeout"),
        Error::Reqwest(e) if e.is_connect() => tr!("updater.connectFailed"),
        Error::Network(message) => message.clone(),
        other => other.to_string(),
    }
}

/// What the diagnostic log records for a failed signature check.
const SIGNATURE: &str = "signature or certificate validation failed";

/// What the log records about an updater error: fixed English words, never
/// the interface text from `describe` (in Chinese it classifies as nothing
/// but "operation failed").
fn cause(e: &Error) -> Cow<'static, str> {
    match e {
        _ if signature_error(e) => SIGNATURE.into(),
        Error::ReleaseNotFound => "not found".into(),
        Error::Reqwest(e) => format!(
            "http_status={:?}; timeout={}; connect={}; body={}; causes={}",
            e.status().map(|s| s.as_u16()),
            e.is_timeout(),
            e.is_connect(),
            e.is_body(),
            argus_core::diagnostics::error_chain(e)
        )
        .into(),
        // The plugin's own words for a download the server refused.
        Error::Network(message) => match message.split_once("status: ") {
            Some((_, status))
                if status.len() >= 3 && status.as_bytes()[..3].iter().all(u8::is_ascii_digit) =>
            {
                format!("http status {}", &status[..3]).into()
            }
            _ => "network error".into(),
        },
        // The plugin's other errors are in English; only their class is kept.
        other => argus_core::diagnostics::failure(&other.to_string()),
    }
}

/// A source that failed: what the dialog says and what the log records.
struct Failed {
    shown: String,
    logged: Cow<'static, str>,
}

impl Failed {
    fn of(e: &Error) -> Self {
        Self {
            shown: describe(e),
            logged: cause(e),
        }
    }

    /// A source address that does not parse (a proxy written into the settings by hand).
    fn address(shown: String) -> Self {
        Self {
            shown,
            logged: "invalid update address".into(),
        }
    }
}

/// The failures of several sources as one message.
fn join(failures: &[String]) -> String {
    failures
        .iter()
        .cloned()
        .reduce(|first, second| tr!("app.join", first = first, second = second))
        .unwrap_or_default()
}

fn signature_error(e: &Error) -> bool {
    matches!(
        e,
        Error::Minisign(_) | Error::SignedVersionMismatch { .. } | Error::MissingSignedVersion
    )
}

/// Checks a minisign signature the way the updater plugin does, with the
/// version it was signed for (`requireSignedVersion`). The error is for the
/// log only.
fn verify_signed(
    data: &[u8],
    signature: &str,
    pubkey: &str,
    version: &str,
) -> Result<(), &'static str> {
    use base64::Engine;
    use minisign_verify::{PublicKey, Signature};
    let failed = || SIGNATURE;
    // Tauri stores both as base64 of the minisign text files.
    let text = |base64: &str| {
        base64::engine::general_purpose::STANDARD
            .decode(base64.trim())
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .ok_or_else(failed)
    };
    let key = PublicKey::decode(&text(pubkey)?).map_err(|_| failed())?;
    let signature = Signature::decode(&text(signature)?).map_err(|_| failed())?;
    key.verify(data, &signature, true).map_err(|_| failed())?;
    // Only now is the trusted comment, and the version in it, known to be signed.
    let signed = signature
        .trusted_comment()
        .split('\t')
        .find_map(|field| field.strip_prefix("version:"));
    match signed {
        Some(v) if v.trim_start_matches('v') == version.trim_start_matches('v') => Ok(()),
        _ => Err(failed()),
    }
}

/// The signed release notes a manifest carries, if any.
fn signed_notes(raw: &serde_json::Value) -> Option<SignedNotes> {
    raw.get("argus")
        .and_then(|value| serde_json::from_value(value.clone()).ok())
}

/// The changelog entries in signed notes, once they pass the signature. The
/// error is for the log only.
fn open_notes(
    notes: &SignedNotes,
    pubkey: &str,
    version: &str,
) -> Result<Vec<ReleaseEntry>, &'static str> {
    if notes.release_notes.len() > MAX_NOTES_BYTES {
        return Err(SIGNATURE);
    }
    verify_signed(
        notes.release_notes.as_bytes(),
        &notes.signature,
        pubkey,
        version,
    )?;
    let payload: NotesPayload =
        serde_json::from_str(&notes.release_notes).map_err(|_| "invalid or unsupported data")?;
    if payload.version != version {
        return Err(SIGNATURE);
    }
    let mut releases = payload.releases;
    releases.truncate(MAX_RELEASES);
    Ok(releases)
}

/// What the dialog may show about `version`: the signed entries, or plain
/// notes from a manifest that has none. Notes that fail the signature are
/// dropped along with the plain ones beside them.
fn notes_for(
    signed: Option<&SignedNotes>,
    plain: Option<String>,
    pubkey: &str,
    version: &str,
) -> (Option<Vec<ReleaseEntry>>, Option<String>) {
    match signed {
        None => (None, plain),
        Some(signed) => match open_notes(signed, pubkey, version) {
            Ok(releases) => (Some(releases), plain),
            Err(e) => {
                log::warn!("release notes rejected: {e}");
                (None, None)
            }
        },
    }
}

async fn find(
    app: &AppHandle,
    manifest: &str,
    source: Option<&str>,
) -> Result<Option<Update>, Failed> {
    app.updater_builder()
        .endpoints(vec![via(manifest, source).map_err(Failed::address)?])
        .map_err(|e| Failed::of(&e))?
        .timeout(MANIFEST_TIMEOUT)
        .build()
        .map_err(|e| Failed::of(&e))?
        .check()
        .await
        .map_err(|e| Failed::of(&e))
}

/// Checks the chosen channel and downloads a newer release. Does nothing while
/// another check, download or install is under way. With an update already
/// downloaded the check runs quietly: that update stays offered unless a
/// newer one replaces it. What it finds after a reset (another channel) is
/// dropped, and it looks again; nothing it finds changes a starting install.
pub async fn check(app: &AppHandle) {
    // A quiet check leaves the state as it is, so the state alone cannot keep
    // two checks from running at once. Release builds abort on a panic, so
    // `checking` needs no guard against unwinding.
    let Some(mut generation) = app.state::<Updates>().0.lock().unwrap().begin_check() else {
        return;
    };
    loop {
        check_once(app, generation).await;
        let again = app
            .state::<Updates>()
            .0
            .lock()
            .unwrap()
            .end_check(generation);
        match again {
            Some(next) => generation = next,
            None => break,
        }
    }
}

async fn check_once(app: &AppHandle, generation: u64) {
    let mut busy = false;
    let mut quiet = false;
    update_status(app, |inner| {
        // `checking` (a reset during the last pass) and `downloading` are this
        // check's own: `begin_check` lets only one run.
        busy = matches!(inner.status.state, "disabled" | "installing");
        quiet = inner.ready.is_some();
        if !busy && !quiet {
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
    // What each source said: for the dialog, and (`causes`) for the log.
    let mut failures = Vec::new();
    let mut causes = Vec::new();
    let mut found = None;
    for (index, source) in sources.iter().enumerate() {
        match find(app, &manifest, source.as_deref()).await {
            Ok(update) => {
                found = Some((index, update));
                break;
            }
            Err(e) => {
                failures.push(tr!(
                    "updater.sourceFailed",
                    source = label(source.as_deref()),
                    error = e.shown
                ));
                causes.push(e.logged);
            }
        }
    }

    let now = now_ms();
    let Some((index, update)) = found else {
        log::warn!("update check failed: {}", causes.join("; "));
        update_found(app, generation, |inner| {
            inner.next_check = now + RETRY_AFTER_MS;
            // A downloaded update can still be installed.
            if inner.ready.is_none() {
                inner.status.state = "error";
                inner.status.error = Some(join(&failures));
                inner.status.checked_at = Some(now);
            }
        });
        return;
    };
    let source = label(sources[index].as_deref());
    let Some(update) = update else {
        // Nothing newer than this version: a kept installer is no longer
        // offered (a withdrawn release).
        let mut had = false;
        update_found(app, generation, |inner| {
            had = inner.ready.take().is_some();
            inner.next_check = now + CHECK_EVERY_MS;
            inner.status = UpdateStatus {
                state: "upToDate",
                checked_at: Some(now),
                source: Some(source),
                portable: inner.status.portable,
                ..Default::default()
            };
        });
        if had {
            discard(app);
        }
        return;
    };

    log::info!("update available");
    let key = pubkey(app);
    let signed = signed_notes(&update.raw_json);
    let (releases, notes) = notes_for(signed.as_ref(), update.body.clone(), &key, &update.version);
    let date = update.date.map(|d| d.unix_timestamp() * 1000);
    let mut already = false;
    let mut portable = false;
    // The offer a failed download of a newer release falls back to.
    let mut previous = None;
    let applies = update_found(app, generation, |inner| {
        inner.next_check = now + CHECK_EVERY_MS;
        portable = inner.status.portable;
        already = inner
            .ready
            .as_ref()
            .is_some_and(|r| r.version == update.version && r.signature == update.signature);
        if inner.ready.is_some() && !already && inner.status.state == "ready" {
            previous = Some(inner.status.clone());
        }
        let size = if already { inner.status.total } else { None };
        inner.status = UpdateStatus {
            state: if already {
                "ready"
            } else if inner.status.portable {
                "available"
            } else {
                "downloading"
            },
            version: Some(update.version.clone()),
            releases: releases.clone(),
            notes: notes.clone(),
            date,
            received: size.unwrap_or_default(),
            total: size,
            checked_at: Some(now),
            source: Some(source),
            portable: inner.status.portable,
            ..Default::default()
        };
    });
    if !applies || already || portable {
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
    causes.clear();
    for source in order {
        let mut attempt = update.clone();
        attempt.download_url = match via(&original, source.as_deref()) {
            Ok(url) => url,
            Err(e) => {
                let e = Failed::address(e);
                failures.push(e.shown);
                causes.push(e.logged);
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
                        update_found(app, generation, |inner| {
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
                let pending = Pending {
                    version: update.version.clone(),
                    channel: settings.update_channel,
                    file: String::new(),
                    signature: update.signature.clone(),
                    date,
                    notes: notes.clone(),
                    signed_notes: signed.clone().filter(|_| releases.is_some()),
                    source: Some(host.clone()),
                };
                match keep(app, pending, &bytes) {
                    Ok(ready) => {
                        let kept = update_found(app, generation, |inner| {
                            inner.ready = Some(ready);
                            inner.status.state = "ready";
                            inner.status.received = bytes.len() as u64;
                            inner.status.total = Some(bytes.len() as u64);
                            inner.status.source = Some(host);
                        });
                        if kept {
                            log::info!("update downloaded");
                        } else if app.state::<Updates>().0.lock().unwrap().generation != generation
                        {
                            // Of the channel before a reset, which removed
                            // the folder already; only this check wrote it
                            // since, and it runs again (`check`).
                            discard(app);
                        }
                        return;
                    }
                    Err(e) => {
                        // An io error keeps its code in any language.
                        causes.push(
                            format!("not kept: {}", argus_core::diagnostics::failure(&e)).into(),
                        );
                        failures.push(tr!("updater.saveFailed", error = e));
                        break;
                    }
                }
            }
            Err(e) => {
                let failed = Failed::of(&e);
                failures.push(tr!(
                    "updater.sourceFailed",
                    source = label(source.as_deref()),
                    error = failed.shown
                ));
                causes.push(failed.logged);
                // A bad artifact is bad from every source.
                if signature_error(&e) {
                    break;
                }
            }
        }
    }
    log::warn!("update download failed: {}", causes.join("; "));
    update_found(app, generation, |inner| {
        inner.next_check = now + RETRY_AFTER_MS;
        match previous.take().filter(|_| inner.ready.is_some()) {
            Some(offer) => inner.status = offer,
            None => {
                inner.status.state = "error";
                inner.status.error = Some(tr!("updater.downloadFailed", error = join(&failures)));
            }
        }
    });
}

fn updates_dir(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_local_data_dir()
        .ok()
        .map(|dir| dir.join(UPDATES_DIR))
}

/// A version is only ever part of a file name; anything else stays out.
fn file_name(version: &str) -> Option<String> {
    (!version.is_empty()
        && version
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+')))
    .then(|| format!("Argus_{version}_x64-setup.exe"))
}

/// Saves a verified installer for later. An older one stays until the new one
/// and its record are written, so a failure leaves the older offer intact.
fn keep(app: &AppHandle, mut pending: Pending, bytes: &[u8]) -> Result<Ready, String> {
    let dir = updates_dir(app).ok_or_else(|| tr!("updater.noDataFolder"))?;
    let name = file_name(&pending.version).ok_or_else(|| tr!("updater.invalidVersion"))?;
    fs::create_dir_all(&dir).map_err(|e| {
        argus_core::diagnostics::file_failure("update cache directory creation", &dir, &e);
        e.to_string()
    })?;
    let path = dir.join(&name);
    write_new(&path, bytes)?;
    pending.file = name.clone();
    let record = serde_json::to_vec(&pending).map_err(|e| e.to_string())?;
    if let Err(e) = write_new(&dir.join(PENDING), &record) {
        let _ = fs::remove_file(&path);
        return Err(e);
    }
    for entry in fs::read_dir(&dir).into_iter().flatten().flatten() {
        let other = entry.file_name();
        if other != name.as_str() && other != PENDING {
            let _ = fs::remove_file(entry.path());
        }
    }
    Ok(Ready {
        version: pending.version,
        path,
        signature: pending.signature,
    })
}

/// Writes through a temporary name, so a crash leaves no half file behind.
fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let partial = path.with_extension("partial");
    let mut stage = "update cache write temporary file";
    fs::write(&partial, bytes)
        .and_then(|_| {
            stage = "update cache replace file";
            fs::rename(&partial, path)
        })
        .map_err(|e| {
            argus_core::diagnostics::warn(
                stage,
                format!(
                    "{}; temporary_path={}; destination_path={}",
                    argus_core::diagnostics::io(&e),
                    argus_core::diagnostics::path(&partial),
                    argus_core::diagnostics::path(path)
                ),
            );
            let _ = fs::remove_file(&partial);
            e.to_string()
        })
}

/// Removes the kept installer. Right after an update the old installer may
/// still be running for a moment; the next start tries again.
fn discard(app: &AppHandle) {
    if let Some(dir) = updates_dir(app)
        && let Err(error) = fs::remove_dir_all(&dir)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        argus_core::diagnostics::file_failure("update cache cleanup", &dir, &error);
    }
}

fn newer(version: &str, than: &str) -> bool {
    match (
        semver::Version::parse(version),
        semver::Version::parse(than),
    ) {
        (Ok(a), Ok(b)) => a > b,
        _ => false,
    }
}

/// Offers an installer kept from an earlier run after checking it again, or
/// removes it once its version is installed or no longer belongs to the
/// chosen channel.
fn restore(app: &AppHandle) {
    let Some(dir) = updates_dir(app) else { return };
    let generation = app.state::<Updates>().0.lock().unwrap().generation;
    let Ok(text) = fs::read_to_string(dir.join(PENDING)) else {
        if dir.exists() {
            discard(app);
        }
        return;
    };
    let channel = app.state::<SettingsStore>().get().update_channel;
    let kept = serde_json::from_str::<Pending>(&text).ok().filter(|p| {
        p.channel == channel
            && newer(&p.version, env!("CARGO_PKG_VERSION"))
            && file_name(&p.version).as_deref() == Some(p.file.as_str())
    });
    let Some(pending) = kept else {
        discard(app);
        return;
    };
    let path = dir.join(&pending.file);
    let key = pubkey(app);
    let size = match fs::read(&path)
        .map_err(|e| {
            argus_core::diagnostics::file_failure("kept update read", &path, &e);
            Cow::Owned(argus_core::diagnostics::io(&e))
        })
        .and_then(|bytes| {
            verify_signed(&bytes, &pending.signature, &key, &pending.version)
                .map(|_| bytes.len() as u64)
                .map_err(Cow::from)
        }) {
        Ok(size) => size,
        Err(e) => {
            log::warn!("kept update dropped: {e}");
            discard(app);
            return;
        }
    };
    let (releases, notes) = notes_for(
        pending.signed_notes.as_ref(),
        pending.notes.clone(),
        &key,
        &pending.version,
    );
    update_status(app, |inner| {
        // A check that already got further wins, and a reset (another
        // channel) meanwhile removed the installer.
        if inner.status.state != "idle" || inner.generation != generation {
            return;
        }
        inner.ready = Some(Ready {
            version: pending.version.clone(),
            path,
            signature: pending.signature.clone(),
        });
        inner.status = UpdateStatus {
            state: "ready",
            version: Some(pending.version),
            releases,
            notes,
            date: pending.date,
            received: size,
            total: Some(size),
            source: pending.source,
            portable: inner.status.portable,
            ..Default::default()
        };
    });
}

/// Starts the downloaded installer, which exits Argus and relaunches it.
/// Returns only when it did not start. Must not run on the main thread.
pub fn install(app: &AppHandle) -> Result<(), InstallError> {
    let updates = app.state::<Updates>();
    let ready = {
        let mut inner = updates.0.lock().unwrap();
        let ready = inner
            .start_install()
            .ok_or_else(|| InstallError::failed(tr!("updater.nothingReady")))?;
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
            let _ = fs::create_dir_all(dir);
        }
        let _ = fs::write(marker, b"");
    }

    log::info!("installing update");
    let failure = launch(app, &ready);
    if let Ok(marker) = &marker {
        let _ = fs::remove_file(marker);
    }
    let error = match &failure {
        Failure::Cancelled => {
            log::info!("update installation cancelled at the administrator prompt");
            InstallError {
                cancelled: true,
                message: tr!("updater.cancelled"),
            }
        }
        Failure::Changed => {
            log::warn!("kept update changed on disk; downloading it again");
            InstallError::failed(tr!("updater.keptChanged"))
        }
        Failure::Other { shown, logged } => {
            log::error!("could not start the update installer: {logged}");
            InstallError::failed(tr!("updater.installerFailed", error = shown))
        }
    };
    let changed = matches!(failure, Failure::Changed);
    update_status(app, |inner| {
        inner.install_failed(changed, (!error.cancelled).then(|| error.message.clone()))
    });
    if changed {
        discard(app);
        let handle = app.clone();
        tauri::async_runtime::spawn(async move { check(&handle).await });
    }
    Err(error)
}

/// Why the installer did not start.
enum Failure {
    /// The administrator prompt was declined.
    Cancelled,
    /// The kept installer no longer passes the signature.
    Changed,
    /// What the dialog says, and what the log records.
    Other {
        shown: String,
        logged: Cow<'static, str>,
    },
}

impl Failure {
    /// An io error's text carries its code ("(os error 5)") in any language.
    fn io(e: &std::io::Error) -> Self {
        let shown = e.to_string();
        Self::Other {
            logged: argus_core::diagnostics::failure(&shown),
            shown,
        }
    }

    /// A Windows error's message is in the system's language; its code is not.
    fn system(e: &windows::core::Error) -> Self {
        Self::Other {
            shown: e.message(),
            logged: argus_core::diagnostics::failure(&format!("{:#010x}", e.code().0 as u32)),
        }
    }
}

/// Checks the kept installer once more and runs it the way the updater plugin
/// would: passive, in update mode, relaunching Argus as the user with our own
/// arguments (the current ones may carry a one-time task manager handshake).
/// Exits the process when the installer started.
fn launch(app: &AppHandle, ready: &Ready) -> Failure {
    use std::io::Read;
    use std::os::windows::fs::OpenOptionsExt;
    use windows::Win32::Foundation::ERROR_CANCELLED;
    use windows::Win32::Storage::FileSystem::FILE_SHARE_READ;
    use windows::Win32::UI::Shell::{
        SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW, ShellExecuteExW,
    };
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    use windows::core::{HSTRING, PCWSTR, w};

    // The folder belongs to the user: hold the file against changes from the
    // check until the installer has started.
    let mut file = match fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ.0)
        .open(&ready.path)
    {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Failure::Changed,
        Err(e) => {
            argus_core::diagnostics::file_failure("update installer open", &ready.path, &e);
            return Failure::io(&e);
        }
    };
    let mut bytes = Vec::new();
    if let Err(e) = file.read_to_end(&mut bytes) {
        argus_core::diagnostics::file_failure("update installer read", &ready.path, &e);
        return Failure::io(&e);
    }
    if verify_signed(&bytes, &ready.signature, &pubkey(app), &ready.version).is_err() {
        return Failure::Changed;
    }
    drop(bytes);

    let path = HSTRING::from(ready.path.as_os_str());
    let parameters = HSTRING::from(format!("/P /UPDATE /R /ARGS {UPDATED_ARG}"));
    let mut info = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
        lpVerb: w!("open"),
        lpFile: PCWSTR(path.as_ptr()),
        lpParameters: PCWSTR(parameters.as_ptr()),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    if let Err(e) = unsafe { ShellExecuteExW(&mut info) } {
        return if e.code() == ERROR_CANCELLED.to_hresult() {
            Failure::Cancelled
        } else {
            argus_core::diagnostics::warn(
                "update installer ShellExecuteExW",
                format!(
                    "{}; path={}",
                    argus_core::diagnostics::code(e.code().0 as u32),
                    argus_core::diagnostics::path(&ready.path)
                ),
            );
            Failure::system(&e)
        };
    }
    drop(file);
    // std::process::exit skips RunEvent::Exit.
    crate::stop_network_session();
    log::logger().flush();
    std::process::exit(0);
}

/// Whether the app was relaunched by an update started from the tray; clears the marker.
pub fn take_relaunch_hidden(app: &AppHandle) -> bool {
    app.path()
        .app_local_data_dir()
        .map(|dir| fs::remove_file(dir.join(RELAUNCH_HIDDEN)).is_ok())
        .unwrap_or(false)
}

/// Opens the release page of `version` (the standalone exe updates by hand).
pub fn open_release_page(version: &str) -> Result<(), String> {
    if !version
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'))
    {
        return Err(tr!("updater.invalidVersion"));
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

    #[test]
    fn kept_installers_have_plain_names() {
        assert_eq!(
            file_name("0.2.0-beta.1").as_deref(),
            Some("Argus_0.2.0-beta.1_x64-setup.exe")
        );
        assert_eq!(file_name("../x"), None);
        assert_eq!(file_name("1.0\\x"), None);
        assert_eq!(file_name(""), None);
    }

    // Signed with a throwaway key (`tauri signer generate`), never a release key.
    const TEST_KEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDJDNkMyMzZCOTYwN0Q2MTAKUldRUTFnZVdheU5zTElwbGNISW5BM004RDlaYWhmZ1ZRNkFONXFqdmI0YnY1dnZTMnFhWWdiU3gK";
    const DATA: &[u8] = b"argus signed data";
    /// `DATA`, signed for version 1.2.3.
    const DATA_SIG: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVRUTFnZVdheU5zTExIN1c3aUMxUE10KzNRblg5eWwvb1J3aFUyZ1VTR1AvdVdTVkxKU1NvL2VRSldYd1FuMjFOUCt6V1dZRmNMNllOM3dwOWdWM3Y2SDRFY2tPekxTM0FVPQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzkyMTg2NDc3CWZpbGU6ZGF0YS5iaW4JdmVyc2lvbjoxLjIuMwpqVW1oV3FjNEMvSnFqdHJ3bjFYOVVEMlQ3RnY4TVE4eUg2WVN4YXhQeGtTSWhkWXk5SkZ6cFlVaU1LQUNpSGZyYmUwLy9YWHRjYWFOYmhuM0Ixd2hBdz09Cg==";
    /// `DATA`, signed without a version.
    const DATA_SIG_UNVERSIONED: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVRUTFnZVdheU5zTEUvbURUZlN0SGlaRGdENVVoNkhHR0N2WDhFeTNQTXc4d2w1YWowTER1QmpvL0pNck51L3YxTmkwOUtid0ZWa0tmL1JxRFA2MlFXR3pBU3IwNGhXUWd3PQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzkyMTg2NDgwCWZpbGU6ZGF0YS5iaW4KMW56UFRSTUlyNC9wcFI5aS8wd09xcVZPemp5K2c2VFVwU09CMWRpelpEMlFlSG0yc1ZEMnoxanNYcmROUUd4VFhKeTM2YkxhRTRyMjQyOUFuM0lHRGc9PQo=";
    const NOTES: &str = r####"{"version":"1.2.3","releases":[{"version":"1.2.3","date":"2026-10-20","notes":"### Fixes\n\n- A fix"},{"version":"1.2.2","date":"2026-10-01","notes":"- Older"}]}"####;
    /// `NOTES`, signed for version 1.2.3.
    const NOTES_SIG: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVRUTFnZVdheU5zTE5IZ3o5cVhFN0RzaEV0U2dYM0ZsMXlQMHZodjhlNThjdG9NT2Fnb1JDSEVROWpTRG9CWWwrY0RZd1hkeWhZOHlYVmJnUklaaFRXZjFJcEt2UHZLbXdVPQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzkyMTg2NDkxCWZpbGU6bm90ZXMuanNvbgl2ZXJzaW9uOjEuMi4zCjlrWXp3SUNUazB3MGJ2ZmI5L1psVUtKUkhHMjFRQ1Z3SzhJOVlUT05rUWl5Zmg2Z2VHaU52aFE3YXY3RG9TYm5yNEYxWHJiQWhONUxJc21oMWkrSEFnPT0K";

    #[test]
    fn signatures_cover_the_data_and_the_version() {
        assert!(verify_signed(DATA, DATA_SIG, TEST_KEY, "1.2.3").is_ok());
        assert!(verify_signed(DATA, DATA_SIG, TEST_KEY, "v1.2.3").is_ok());
        assert!(verify_signed(b"argus signed datA", DATA_SIG, TEST_KEY, "1.2.3").is_err());
        // Another version's signature cannot vouch for this one.
        assert!(verify_signed(DATA, DATA_SIG, TEST_KEY, "1.2.4").is_err());
        assert!(verify_signed(DATA, DATA_SIG_UNVERSIONED, TEST_KEY, "1.2.3").is_err());
        assert!(verify_signed(DATA, "not base64", TEST_KEY, "1.2.3").is_err());
    }

    #[test]
    fn release_notes_show_only_when_signed() {
        let signed = SignedNotes {
            release_notes: NOTES.into(),
            signature: NOTES_SIG.into(),
        };
        let releases = open_notes(&signed, TEST_KEY, "1.2.3").unwrap();
        assert_eq!(releases.len(), 2);
        assert_eq!(releases[0].version, "1.2.3");
        assert_eq!(releases[1].notes, "- Older");
        assert!(open_notes(&signed, TEST_KEY, "1.2.4").is_err());

        let altered = SignedNotes {
            release_notes: NOTES.replace("A fix", "A fiX"),
            signature: NOTES_SIG.into(),
        };
        assert!(open_notes(&altered, TEST_KEY, "1.2.3").is_err());
        // Altered notes take the unsigned text beside them down too.
        assert_eq!(
            notes_for(Some(&altered), Some("plain".into()), TEST_KEY, "1.2.3"),
            (None, None)
        );
        assert_eq!(
            notes_for(None, Some("plain".into()), TEST_KEY, "1.2.3"),
            (None, Some("plain".into()))
        );

        let manifest = serde_json::json!({
            "version": "1.2.3",
            "argus": { "releaseNotes": NOTES, "signature": NOTES_SIG },
        });
        let found = signed_notes(&manifest).unwrap();
        assert_eq!(found.release_notes, NOTES);
        assert!(signed_notes(&serde_json::json!({ "version": "1.2.3" })).is_none());
    }

    #[test]
    fn release_notes_carry_their_translations() {
        let payload: NotesPayload = serde_json::from_str(
            r#"{"version":"1.2.3","releases":[
                {"version":"1.2.3","date":"2026-10-21","notes":"- 修复","translations":{"en-US":"- A fix"}},
                {"version":"1.2.2","date":"2026-10-12","notes":"- 旧版"}
            ]}"#,
        )
        .unwrap();
        assert_eq!(payload.releases[0].translations["en-US"], "- A fix");
        // Notes signed before translations existed still read.
        assert!(payload.releases[1].translations.is_empty());
        // The interface gets them; an entry without any has no field for them.
        let sent = serde_json::to_value(&payload.releases).unwrap();
        assert_eq!(sent[0]["translations"]["en-US"], "- A fix");
        assert!(sent[1].get("translations").is_none());
    }

    #[test]
    fn the_log_gets_fixed_words_in_any_interface_language() {
        // The dialog's text (`describe`) is in the interface language; the log
        // classifies the error itself.
        for (error, logged) in [
            (Error::MissingSignedVersion, SIGNATURE),
            (
                Error::SignedVersionMismatch {
                    signed: "1.2.3".into(),
                    announced: "1.2.4".into(),
                },
                SIGNATURE,
            ),
            (Error::ReleaseNotFound, "not found"),
            (
                Error::Network(
                    "Download request failed with status: 503 Service Unavailable".into(),
                ),
                "http status 503",
            ),
            (
                Error::Network("Download request failed with status: 404 Not Found".into()),
                "http status 404",
            ),
            (Error::Network("status: 50".into()), "network error"),
            (Error::Network("connection reset".into()), "network error"),
        ] {
            assert_eq!(cause(&error), logged);
        }
        assert_eq!(
            verify_signed(DATA, DATA_SIG, TEST_KEY, "1.2.4"),
            Err(SIGNATURE)
        );
        let altered = SignedNotes {
            release_notes: NOTES.replace("A fix", "A fiX"),
            signature: NOTES_SIG.into(),
        };
        assert_eq!(
            open_notes(&altered, TEST_KEY, "1.2.3").err(),
            Some(SIGNATURE)
        );

        // An installer that did not start: its error code, never the system's
        // message around it.
        let Failure::Other { logged, .. } = Failure::io(&std::io::Error::from_raw_os_error(5))
        else {
            panic!("an io error is another failure");
        };
        assert_eq!(logged, "access denied (os error 5)");
        let e = windows::core::Error::from_hresult(windows::core::HRESULT(0x8007_0002_u32 as i32));
        let Failure::Other { logged, .. } = Failure::system(&e) else {
            panic!("a Windows error is another failure");
        };
        assert_eq!(logged, "not found (os error 2)");
    }

    fn ready(version: &str) -> Inner {
        let mut inner = Inner::new(UpdateStatus {
            state: "ready",
            version: Some(version.into()),
            ..Default::default()
        });
        inner.ready = Some(Ready {
            version: version.into(),
            path: PathBuf::from(version),
            signature: String::new(),
        });
        inner
    }

    #[test]
    fn a_check_drops_what_it_finds_after_a_channel_switch_and_looks_again() {
        let mut inner = ready("0.2.0");
        let first = inner.begin_check().unwrap();
        assert_eq!(inner.begin_check(), None, "one check at a time");
        // Switching channels while it fetches: the kept update goes, and the
        // check's result for the old channel no longer applies.
        assert!(inner.reset());
        assert!(inner.ready.is_none());
        assert_eq!(inner.status.state, "checking");
        assert!(!inner.current(first));
        // It runs again for the new channel, then ends.
        let second = inner.end_check(first).unwrap();
        assert!(inner.current(second));
        assert_eq!(inner.end_check(second), None);
        assert_eq!(inner.begin_check(), Some(second));
        assert_eq!(inner.end_check(second), None);
        // With no check running, a reset just goes idle.
        assert!(inner.reset());
        assert_eq!(inner.status.state, "idle");
    }

    #[test]
    fn checks_and_resets_leave_a_starting_installer_alone() {
        let mut inner = ready("0.2.0");
        let generation = inner.begin_check().unwrap();
        let installing = inner.start_install().unwrap();
        assert_eq!(installing.version, "0.2.0");
        assert_eq!(inner.status.state, "installing");
        assert!(inner.start_install().is_none(), "installing already");
        // A newer release found by the quiet check meanwhile is not applied,
        // nor taken as the offer to fall back to.
        assert!(!inner.current(generation));
        assert!(!inner.reset());
        assert_eq!(inner.end_check(generation), None);
        // The administrator prompt declined: the same update offered again.
        inner.install_failed(false, None);
        assert_eq!(inner.status.state, "ready");
        assert_eq!(inner.status.version.as_deref(), Some("0.2.0"));
        assert_eq!(inner.ready.as_ref().unwrap().version, "0.2.0");
        // Only an installing state is ended by the install.
        inner.install_failed(true, Some("changed".into()));
        assert_eq!(inner.status.state, "ready");
        inner.start_install().unwrap();
        inner.install_failed(true, Some("changed".into()));
        assert_eq!(inner.status.state, "error");
        assert!(inner.ready.is_none());
    }

    #[test]
    fn versions_compare_as_semver() {
        assert!(newer("0.2.0", "0.1.0"));
        assert!(newer("0.2.0", "0.2.0-beta.2"));
        assert!(newer("0.2.0-beta.10", "0.2.0-beta.2"));
        assert!(!newer("0.1.0", "0.1.0"));
        assert!(!newer("0.1.0", "0.2.0"));
        assert!(!newer("x", "0.1.0"));
    }
}
