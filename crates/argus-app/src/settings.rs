//! User settings that the native side needs before the UI loads (language,
//! theme, hotkey, close behavior, autostart, process rules, history recording,
//! updates and what a new version shows first, onboarding), persisted as JSON
//! in the app config dir.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use argus_core::i18n::Locale;
use argus_core::rules::ProcessRule;
use serde::{Deserialize, Serialize};

pub const DEFAULT_HOTKEY: &str = "Ctrl+Alt+Escape";
/// The cadences on offer while minimized or in the tray, ms (Settings and the
/// onboarding offer the same list).
pub const BACKGROUND_INTERVALS: [u64; 6] = [500, 1000, 2000, 5000, 10_000, 30_000];
pub const DEFAULT_BACKGROUND_INTERVAL: u64 = 5000;

/// The UI language: a locale, or the Windows display language. Stored as the
/// tag; one this version does not know (from a newer Argus) reads as `System`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum Language {
    #[default]
    System,
    Locale(Locale),
}

impl Language {
    pub fn resolve(self) -> Locale {
        match self {
            Language::System => Locale::system(),
            Language::Locale(locale) => locale,
        }
    }
}

impl From<String> for Language {
    fn from(tag: String) -> Self {
        Locale::from_tag(&tag).map_or(Language::System, Language::Locale)
    }
}

impl From<Language> for String {
    fn from(language: Language) -> Self {
        match language {
            Language::System => "system".into(),
            Language::Locale(locale) => locale.tag().into(),
        }
    }
}

/// The color theme: light, dark, or the Windows app mode. Stored as its name;
/// one this version does not know reads as `System`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

impl From<String> for Theme {
    fn from(name: String) -> Self {
        match name.as_str() {
            "light" => Theme::Light,
            "dark" => Theme::Dark,
            _ => Theme::System,
        }
    }
}

impl From<Theme> for String {
    fn from(theme: Theme) -> Self {
        match theme {
            Theme::System => "system",
            Theme::Light => "light",
            Theme::Dark => "dark",
        }
        .into()
    }
}

/// What the tray icon shows: the app's icon, or the CPU load live as the
/// recent samples (`Load`), as the share of a hundred dots, or as a figure.
/// Stored as its name; one this version does not know reads as the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum TrayIcon {
    Logo,
    #[default]
    Load,
    Share,
    Number,
}

impl From<String> for TrayIcon {
    fn from(name: String) -> Self {
        match name.as_str() {
            "logo" => TrayIcon::Logo,
            "share" => TrayIcon::Share,
            "number" => TrayIcon::Number,
            _ => TrayIcon::Load,
        }
    }
}

impl From<TrayIcon> for String {
    fn from(icon: TrayIcon) -> Self {
        match icon {
            TrayIcon::Logo => "logo",
            TrayIcon::Load => "load",
            TrayIcon::Share => "share",
            TrayIcon::Number => "number",
        }
        .into()
    }
}

/// What a right click on the tray icon opens. Stored as its name; one this
/// version does not know reads as the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum TrayMenu {
    /// The quick panel: readings, the busiest processes and common actions.
    #[default]
    Panel,
    /// The system menu with the common actions only.
    Menu,
}

impl From<String> for TrayMenu {
    fn from(name: String) -> Self {
        match name.as_str() {
            "menu" => TrayMenu::Menu,
            _ => TrayMenu::Panel,
        }
    }
}

impl From<TrayMenu> for String {
    fn from(menu: TrayMenu) -> Self {
        match menu {
            TrayMenu::Panel => "panel",
            TrayMenu::Menu => "menu",
        }
        .into()
    }
}

/// A reading the desktop and taskbar monitors can show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Metric {
    /// CPU load, percent.
    Cpu,
    CpuTemperature,
    /// The CPU's clock speed.
    CpuClock,
    /// The CPU package's power, or its cores' where there is no package figure.
    CpuPower,
    /// Memory in use, percent.
    Memory,
    /// Memory in use, bytes.
    MemoryUsed,
    Gpu,
    GpuTemperature,
    /// Dedicated GPU memory in use, percent.
    GpuMemory,
    GpuClock,
    GpuPower,
    GpuFan,
    /// Read and write rates of all disks.
    Disk,
    /// The busiest disk's active time.
    DiskActive,
    /// The hottest disk's temperature.
    DiskTemperature,
    /// Receive and send rates of all adapters.
    Network,
    /// The battery's charge.
    Battery,
    /// Time since Windows started.
    Uptime,
}

impl Metric {
    /// Every metric, in the order the components page offers them and the
    /// monitors show them: by device.
    pub const ALL: [Metric; 18] = [
        Metric::Cpu,
        Metric::CpuTemperature,
        Metric::CpuClock,
        Metric::CpuPower,
        Metric::Memory,
        Metric::MemoryUsed,
        Metric::Gpu,
        Metric::GpuTemperature,
        Metric::GpuMemory,
        Metric::GpuClock,
        Metric::GpuPower,
        Metric::GpuFan,
        Metric::Disk,
        Metric::DiskActive,
        Metric::DiskTemperature,
        Metric::Network,
        Metric::Battery,
        Metric::Uptime,
    ];
}

impl Metric {
    /// The metric reads one device of several when given one: a GPU, a
    /// disk or a network adapter.
    pub fn per_device(self) -> bool {
        matches!(
            self,
            Metric::Gpu
                | Metric::GpuTemperature
                | Metric::GpuMemory
                | Metric::GpuClock
                | Metric::GpuPower
                | Metric::GpuFan
                | Metric::Disk
                | Metric::DiskActive
                | Metric::DiskTemperature
                | Metric::Network
        )
    }

    /// Its saved name, `cpuTemperature` say.
    pub fn name(self) -> String {
        match serde_json::to_value(self) {
            Ok(serde_json::Value::String(name)) => name,
            _ => String::new(),
        }
    }
}

/// A reading a monitor shows: a metric, of one device or of the default
/// one. Without a device, GPU readings follow the GPU with the most
/// dedicated memory, disk and network rates add up every disk and adapter,
/// and a disk's active time and temperature are the busiest and the hottest
/// disk's. Saved as the metric's name, followed by `@` and the device's key
/// when it has one: `diskTemperature@disk:nvme|Samsung SSD 990 PRO 2TB|2000398934016`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Reading {
    pub metric: Metric,
    /// The device's key, as the interface makes it from what the device
    /// reports (its model, its name), so that it outlasts restarts.
    pub device: Option<String>,
}

impl From<Metric> for Reading {
    fn from(metric: Metric) -> Self {
        Self {
            metric,
            device: None,
        }
    }
}

impl Reading {
    /// A reading as saved; none for a metric this version does not know. A
    /// device on a metric that reads none is left out.
    pub fn parse(text: &str) -> Option<Self> {
        let (name, device) = match text.split_once('@') {
            Some((name, device)) => (name, Some(device)),
            None => (text, None),
        };
        let metric: Metric = serde_json::from_value(serde_json::Value::String(name.into())).ok()?;
        Some(Self {
            metric,
            device: device
                .filter(|device| !device.is_empty() && metric.per_device())
                .map(str::to_owned),
        })
    }

    pub fn text(&self) -> String {
        match &self.device {
            Some(device) => format!("{}@{device}", self.metric.name()),
            None => self.metric.name(),
        }
    }
}

impl Serialize for Reading {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.text())
    }
}

impl<'de> Deserialize<'de> for Reading {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        Reading::parse(&text)
            .ok_or_else(|| serde::de::Error::custom(format!("unknown reading {text}")))
    }
}

/// A list of readings as saved: ones this version does not know (from a
/// newer Argus, or a hand edit) and repeats are left out, keeping the rest.
fn lenient_metrics<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<Reading>, D::Error> {
    let mut readings = Vec::new();
    if let serde_json::Value::Array(names) = serde_json::Value::deserialize(d)? {
        for name in names {
            if let Some(reading) = name.as_str().and_then(Reading::parse)
                && !readings.contains(&reading)
            {
                readings.push(reading);
            }
        }
    }
    Ok(readings)
}

/// An order of metrics as saved: ones this version does not know and
/// repeats are left out, keeping the rest.
fn lenient_order<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<Metric>, D::Error> {
    let mut order = Vec::new();
    if let serde_json::Value::Array(names) = serde_json::Value::deserialize(d)? {
        for name in names {
            if let Ok(metric) = serde_json::from_value::<Metric>(name)
                && !order.contains(&metric)
            {
                order.push(metric);
            }
        }
    }
    Ok(order)
}

/// The window opacities the desktop monitor offers, percent.
pub const MONITOR_OPACITIES: [u8; 5] = [100, 90, 80, 70, 60];

/// Settings stored by name: a name this version does not know reads as the
/// default.
macro_rules! named {
    ($(#[$meta:meta])* $name:ident { $($(#[$variant_meta:meta])* $variant:ident = $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
        #[serde(from = "String", into = "String")]
        pub enum $name {
            $($(#[$variant_meta])* $variant),+
        }

        impl From<String> for $name {
            fn from(name: String) -> Self {
                match name.as_str() {
                    $($text => $name::$variant,)+
                    _ => $name::default(),
                }
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                match value {
                    $($name::$variant => $text,)+
                }
                .into()
            }
        }
    };
}

named! {
    /// How the desktop monitor lays its readings out.
    DeskLayout {
        /// A panel of groups, one device's readings to a group.
        #[default]
        Panel = "panel",
        /// One line along a screen edge, wrapping between groups where the
        /// display is too narrow.
        Strip = "strip",
    }
}

named! {
    /// How the desktop monitor's panel shows a group.
    DeskStyle {
        /// Names, bars and values.
        #[default]
        List = "list",
        /// Names and values beside the group's recent curve.
        Curves = "curves",
        /// A tile a group, a ring for its first reading.
        Gauges = "gauges",
    }
}

named! {
    /// How the panel's list and curves set one group apart from the next.
    DeskGrouping {
        #[default]
        Cards = "cards",
        /// A strip in the device's colour before each group.
        Stripes = "stripes",
        /// A line between groups.
        Rules = "rules",
    }
}

/// The desktop monitor: a small window of live readings that stays on the
/// desktop outside the main window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DesktopMonitor {
    pub enabled: bool,
    /// What it shows, in this order.
    #[serde(deserialize_with = "lenient_metrics")]
    pub metrics: Vec<Reading>,
    /// How the kinds of reading (CPU, memory, GPU, disk, network, the rest)
    /// and each kind's metrics were arranged on the components page: the
    /// kinds in the order of their first metrics here. Metrics it leaves
    /// out go after the others of their kind, in the order offered
    /// (`Metric::ALL`); empty, everything is in that order. `metrics`
    /// keeps to it.
    #[serde(deserialize_with = "lenient_order")]
    pub order: Vec<Metric>,
    pub layout: DeskLayout,
    pub style: DeskStyle,
    pub grouping: DeskGrouping,
    /// It stays above other windows.
    pub topmost: bool,
    /// It cannot be dragged.
    pub locked: bool,
    /// Clicks pass through it to the windows below.
    pub click_through: bool,
    /// Percent; one of `MONITOR_OPACITIES`.
    pub opacity: u8,
    /// Its top-left corner on the virtual screen, physical pixels; none
    /// until it was first moved (it then opens at the top right of the
    /// primary display).
    pub position: Option<[i32; 2]>,
}

impl Default for DesktopMonitor {
    fn default() -> Self {
        Self {
            enabled: false,
            metrics: [
                Metric::Cpu,
                Metric::Memory,
                Metric::Gpu,
                Metric::Disk,
                Metric::Network,
            ]
            .map(Reading::from)
            .to_vec(),
            order: Vec::new(),
            layout: DeskLayout::default(),
            style: DeskStyle::default(),
            grouping: DeskGrouping::default(),
            topmost: true,
            locked: false,
            click_through: false,
            opacity: 100,
            position: None,
        }
    }
}

/// Where on the taskbar the taskbar monitor sits. Stored as its name; one
/// this version does not know reads as the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum TaskbarPlace {
    /// Beside the notification area.
    #[default]
    Tray,
    /// At the start of the taskbar; only on Windows 11 with the taskbar's
    /// buttons centred, beside the tray otherwise.
    Start,
}

impl From<String> for TaskbarPlace {
    fn from(name: String) -> Self {
        match name.as_str() {
            "start" => TaskbarPlace::Start,
            _ => TaskbarPlace::Tray,
        }
    }
}

impl From<TaskbarPlace> for String {
    fn from(place: TaskbarPlace) -> Self {
        match place {
            TaskbarPlace::Tray => "tray",
            TaskbarPlace::Start => "start",
        }
        .into()
    }
}

/// How the taskbar monitor shows its readings. Stored as its name; one
/// this version does not know reads as the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum TaskbarStyle {
    /// Names and values in two rows.
    #[default]
    Labels,
    /// A tile per reading: its name and value over its recent curve.
    Curves,
    /// A rounded capsule per reading, marked with a dot in its device's
    /// colour, in one row.
    Capsules,
    /// A small name in its device's colour over a large figure.
    Figures,
}

impl From<String> for TaskbarStyle {
    fn from(name: String) -> Self {
        match name.as_str() {
            "curves" => TaskbarStyle::Curves,
            "capsules" => TaskbarStyle::Capsules,
            "figures" => TaskbarStyle::Figures,
            _ => TaskbarStyle::Labels,
        }
    }
}

impl From<TaskbarStyle> for String {
    fn from(style: TaskbarStyle) -> Self {
        match style {
            TaskbarStyle::Labels => "labels",
            TaskbarStyle::Curves => "curves",
            TaskbarStyle::Capsules => "capsules",
            TaskbarStyle::Figures => "figures",
        }
        .into()
    }
}

/// The taskbar monitor: live readings on the primary display's taskbar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TaskbarMonitor {
    pub enabled: bool,
    /// What it shows, in this order.
    #[serde(deserialize_with = "lenient_metrics")]
    pub metrics: Vec<Reading>,
    /// How its readings were arranged, as the desktop monitor's `order`.
    #[serde(deserialize_with = "lenient_order")]
    pub order: Vec<Metric>,
    pub place: TaskbarPlace,
    pub style: TaskbarStyle,
    /// The display whose taskbar it sits on, by what keeps that display
    /// apart across restarts (its monitor's device path); empty for the
    /// primary display, whichever that is. While that display is away or
    /// shows no taskbar, it sits on the primary display's.
    pub display: String,
}

impl Default for TaskbarMonitor {
    fn default() -> Self {
        Self {
            enabled: false,
            metrics: [Metric::Cpu, Metric::Memory, Metric::Network]
                .map(Reading::from)
                .to_vec(),
            order: Vec::new(),
            place: TaskbarPlace::default(),
            style: TaskbarStyle::default(),
            display: String::new(),
        }
    }
}

/// A monitor's settings as saved, or its defaults when they do not read
/// (a hand edit), keeping the other settings.
fn lenient_monitor<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned + Default,
{
    Ok(serde_json::from_value(serde_json::Value::deserialize(d)?).unwrap_or_default())
}

/// Which releases count as updates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UpdateChannel {
    #[default]
    Stable,
    /// Prereleases as well as stable releases.
    Beta,
}

/// How update requests reach GitHub.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UpdateRoute {
    /// GitHub first, then the proxies.
    #[default]
    Auto,
    Direct,
    /// Only the chosen proxy.
    Proxy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub language: Language,
    pub theme: Theme,
    /// The onboarding version last completed or skipped; 0 before the first.
    /// The UI owns the numbering (`onboardingVersion` in resources/desktop.json).
    pub onboarding: u32,
    /// The version whose first start has been dealt with: its changes shown
    /// or announced, or nothing to show (a fresh install). Empty until a
    /// version that records it has run.
    pub seen_version: String,
    /// Closing the window keeps Argus running in the tray.
    pub close_to_tray: bool,
    /// What the tray icon shows.
    pub tray_icon: TrayIcon,
    /// Hovering the tray icon shows a card of live readings; off, the
    /// system tooltip shows them.
    pub tray_card: bool,
    /// What a right click on the tray icon opens.
    pub tray_menu: TrayMenu,
    /// How often to sample while the window is minimized, ms; one of
    /// `BACKGROUND_INTERVALS`.
    pub minimized_interval: u64,
    /// How often to sample while Argus is in the tray, ms.
    pub tray_interval: u64,
    #[serde(deserialize_with = "lenient_monitor")]
    pub desktop_monitor: DesktopMonitor,
    #[serde(deserialize_with = "lenient_monitor")]
    pub taskbar_monitor: TaskbarMonitor,
    /// Global shortcut that shows/hides the window, e.g. `Ctrl+Alt+Escape`
    /// (see hotkey.rs); empty disables it.
    #[serde(deserialize_with = "lenient_hotkey")]
    pub hotkey: String,
    /// Whether `hotkey` is registered; turning it off keeps the combination.
    pub hotkey_enabled: bool,
    /// Priority and efficiency mode rules, applied as programs start.
    pub rules: Vec<ProcessRule>,
    /// Record history for the history page.
    pub history: bool,
    /// Opt-in CPU sensors; an explicit enable action prepares PawnIO if needed.
    pub kernel_sensors: bool,
    /// Check for updates in the background.
    pub update_check: bool,
    pub update_channel: UpdateChannel,
    pub update_route: UpdateRoute,
    /// GitHub proxy prefix, e.g. `https://gh-proxy.com/`.
    pub update_proxy: String,
    /// Show what a new version changed on its first start; off, a notice
    /// says it was updated instead.
    pub whats_new: bool,
    /// Draw the interface with the graphics card; off, the system's software
    /// rasterizer draws it on the processor.
    pub hardware_acceleration: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: Language::default(),
            theme: Theme::default(),
            onboarding: 0,
            seen_version: String::new(),
            close_to_tray: true,
            tray_icon: TrayIcon::default(),
            tray_card: true,
            tray_menu: TrayMenu::default(),
            minimized_interval: DEFAULT_BACKGROUND_INTERVAL,
            tray_interval: DEFAULT_BACKGROUND_INTERVAL,
            desktop_monitor: DesktopMonitor::default(),
            taskbar_monitor: TaskbarMonitor::default(),
            hotkey: DEFAULT_HOTKEY.into(),
            hotkey_enabled: true,
            rules: Vec::new(),
            history: true,
            kernel_sensors: false,
            update_check: true,
            update_channel: UpdateChannel::default(),
            update_route: UpdateRoute::default(),
            update_proxy: crate::config::PROXY_PRESETS[0].into(),
            whats_new: true,
            hardware_acceleration: true,
        }
    }
}

impl Settings {
    /// Brings settings saved by an older version up to date, in memory; they
    /// are written back with the next change.
    fn migrate(&mut self) {
        // The hotkey used to be turned off by clearing it.
        if self.hotkey.is_empty() {
            self.hotkey = DEFAULT_HOTKEY.into();
            self.hotkey_enabled = false;
        }
        // A hand edit to an interval on offer nowhere.
        for ms in [&mut self.minimized_interval, &mut self.tray_interval] {
            if !BACKGROUND_INTERVALS.contains(ms) {
                *ms = DEFAULT_BACKGROUND_INTERVAL;
            }
        }
        if !MONITOR_OPACITIES.contains(&self.desktop_monitor.opacity) {
            self.desktop_monitor.opacity = 100;
        }
    }

    /// The sampler's cadences while minimized and while in the tray.
    pub fn background_intervals(&self) -> crate::sampler::Control {
        crate::sampler::Control::SetBackgroundIntervals {
            minimized: Duration::from_millis(self.minimized_interval),
            tray: Duration::from_millis(self.tray_interval),
        }
    }
}

/// A hotkey that is not text (a hand edit) reads as the default instead of
/// sending every other setting back to its default too.
fn lenient_hotkey<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    Ok(match serde_json::Value::deserialize(d)? {
        serde_json::Value::String(hotkey) => hotkey,
        _ => DEFAULT_HOTKEY.into(),
    })
}

/// What the settings page shows: stored settings plus live state.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView<E> {
    #[serde(flatten)]
    pub settings: Settings,
    pub autostart: bool,
    /// Why the hotkey is not active (e.g. taken by another app).
    pub hotkey_error: Option<E>,
    pub default_hotkey: &'static str,
}

pub struct SettingsStore {
    path: PathBuf,
    pub current: Mutex<Settings>,
}

impl SettingsStore {
    pub fn load(path: PathBuf) -> Self {
        let mut current = read(&path).unwrap_or_default();
        current.migrate();
        Self {
            path,
            current: Mutex::new(current),
        }
    }

    pub fn get(&self) -> Settings {
        self.current.lock().unwrap().clone()
    }

    pub fn update(&self, f: impl FnOnce(&mut Settings)) -> std::io::Result<Settings> {
        let mut s = self.current.lock().unwrap();
        let mut next = s.clone();
        f(&mut next);
        write(&self.path, &serde_json::to_vec_pretty(&next)?)?;
        *s = next;
        Ok(s.clone())
    }
}

/// The saved settings; None means defaults. Whatever stands in the way is
/// logged by kind, never by content: settings lost across a restart can then
/// be told apart (no file, an empty or zeroed one, one that does not parse).
/// A file that does not parse is kept as `settings.json.bad`, since the next
/// change would overwrite it.
fn read(path: &Path) -> Option<Settings> {
    let mut retries = 3;
    let bytes = loop {
        match std::fs::read(path) {
            Ok(bytes) => break bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                log::info!("settings: none saved, using defaults");
                return None;
            }
            // Another program (a virus scanner, say) may hold it for a moment;
            // defaults now would overwrite it with the next change.
            Err(_) if retries > 0 => {
                retries -= 1;
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => {
                log::warn!(
                    "settings: not read ({}), path={}, using defaults",
                    argus_core::diagnostics::io(&e),
                    argus_core::diagnostics::path(path)
                );
                return None;
            }
        }
    };
    match serde_json::from_slice::<Settings>(&bytes) {
        Ok(settings) => {
            log::info!("settings: loaded (onboarding {})", settings.onboarding);
            Some(settings)
        }
        Err(e) => {
            let kind = if bytes.is_empty() {
                "empty"
            } else if bytes.iter().all(|&b| b == 0) {
                "all zero bytes"
            } else {
                "not valid"
            };
            log::warn!(
                "settings: {} bytes, {kind} ({:?} error at line {}, column {}), path={}, using defaults",
                bytes.len(),
                e.classify(),
                e.line(),
                e.column(),
                argus_core::diagnostics::path(path)
            );
            if let Err(error) = std::fs::rename(path, path.with_extension("json.bad")) {
                argus_core::diagnostics::warn(
                    "preserve invalid settings",
                    format!(
                        "{}; source={}; destination={}",
                        argus_core::diagnostics::io(&error),
                        argus_core::diagnostics::path(path),
                        argus_core::diagnostics::path(&path.with_extension("json.bad"))
                    ),
                );
            }
            None
        }
    }
}

/// Writes through a temporary file that reaches the disk before it replaces
/// the old one, so a crash, a forced restart or a power cut leaves the old
/// settings or the new ones, never an empty or cut-off file.
pub(crate) fn write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut stage = "create settings directory";
    let result = write_with_stage(path, bytes, &mut stage);
    if let Err(error) = &result {
        argus_core::diagnostics::warn(
            stage,
            format!(
                "{}; path={}",
                argus_core::diagnostics::io(error),
                argus_core::diagnostics::path(path)
            ),
        );
    }
    result
}

fn write_with_stage(path: &Path, bytes: &[u8], stage: &mut &'static str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let partial = path.with_extension("json.partial");
    *stage = "create temporary settings file";
    let result = std::fs::File::create(&partial)
        .and_then(|mut file| {
            *stage = "write temporary settings file";
            file.write_all(bytes)?;
            *stage = "flush settings file";
            file.sync_all()
        })
        .and_then(|_| {
            *stage = "replace settings file";
            std::fs::rename(&partial, path)
        });
    if result.is_err() {
        let _ = std::fs::remove_file(&partial);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_save_does_not_enable_kernel_sensors_in_memory() {
        let id = unsafe { windows::Win32::System::Com::CoCreateGuid() }.unwrap();
        let directory = std::env::temp_dir().join(format!("argus-settings-{id:?}"));
        std::fs::create_dir(&directory).unwrap();
        let store = SettingsStore {
            path: directory.clone(),
            current: Mutex::new(Settings::default()),
        };
        assert!(
            store
                .update(|settings| settings.kernel_sensors = true)
                .is_err()
        );
        assert!(!store.get().kernel_sensors);
        std::fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn a_cleared_hotkey_reads_as_turned_off() {
        let mut settings: Settings = serde_json::from_str(r#"{"hotkey": ""}"#).unwrap();
        settings.migrate();
        assert_eq!(settings.hotkey, DEFAULT_HOTKEY);
        assert!(!settings.hotkey_enabled);

        let mut settings: Settings =
            serde_json::from_str(r#"{"hotkey": "Ctrl+Alt+KeyK"}"#).unwrap();
        settings.migrate();
        assert_eq!(settings.hotkey, "Ctrl+Alt+KeyK");
        assert!(settings.hotkey_enabled);
    }

    #[test]
    fn tray_choices_round_trip_and_unknown_names_read_as_defaults() {
        // Settings saved before the tray choices existed.
        let settings: Settings = serde_json::from_str(r#"{"closeToTray": false}"#).unwrap();
        assert_eq!(settings.tray_icon, TrayIcon::Load);
        assert!(settings.tray_card);
        assert_eq!(settings.tray_menu, TrayMenu::Panel);
        // Names from a newer version keep the other settings.
        let settings: Settings = serde_json::from_str(
            r#"{"trayIcon": "sparkline", "trayMenu": "wheel", "closeToTray": false}"#,
        )
        .unwrap();
        assert_eq!(settings.tray_icon, TrayIcon::Load);
        assert_eq!(settings.tray_menu, TrayMenu::Panel);
        assert!(!settings.close_to_tray);
        for icon in [
            TrayIcon::Logo,
            TrayIcon::Load,
            TrayIcon::Share,
            TrayIcon::Number,
        ] {
            for menu in [TrayMenu::Panel, TrayMenu::Menu] {
                let saved = Settings {
                    tray_icon: icon,
                    tray_card: false,
                    tray_menu: menu,
                    ..Settings::default()
                };
                let read: Settings =
                    serde_json::from_str(&serde_json::to_string(&saved).unwrap()).unwrap();
                assert_eq!(
                    (read.tray_icon, read.tray_card, read.tray_menu),
                    (icon, false, menu)
                );
            }
        }
    }

    #[test]
    fn monitors_start_off_and_keep_what_they_know() {
        // Settings saved before the monitors existed.
        let old: Settings = serde_json::from_str(r#"{"closeToTray": false}"#).unwrap();
        assert_eq!(old.desktop_monitor, DesktopMonitor::default());
        assert_eq!(old.taskbar_monitor, TaskbarMonitor::default());
        assert!(!old.desktop_monitor.enabled && !old.taskbar_monitor.enabled);
        // Unknown and repeated metrics drop out; an unknown place reads as
        // the tray; the rest of the monitor stays.
        let newer: Settings = serde_json::from_str(
            r#"{"desktopMonitor": {"enabled": true, "metrics": ["fps", "network", "cpu", "network"], "position": [-1200, 40]},
                "taskbarMonitor": {"enabled": true, "metrics": [], "place": "middle"}}"#,
        )
        .unwrap();
        assert!(newer.desktop_monitor.enabled);
        assert_eq!(
            newer.desktop_monitor.metrics,
            [Metric::Network, Metric::Cpu].map(Reading::from)
        );
        assert_eq!(newer.desktop_monitor.position, Some([-1200, 40]));
        assert!(newer.desktop_monitor.topmost);
        assert!(newer.taskbar_monitor.enabled);
        assert!(newer.taskbar_monitor.metrics.is_empty());
        assert!(newer.desktop_monitor.order.is_empty() && newer.taskbar_monitor.order.is_empty());
        assert_eq!(newer.taskbar_monitor.place, TaskbarPlace::Tray);
        // An arrangement keeps the metrics it knows, each once.
        let arranged: Settings = serde_json::from_str(
            r#"{"taskbarMonitor": {"order": ["disk", "fps", "cpuPower", "disk", 3, "cpu"]}}"#,
        )
        .unwrap();
        assert_eq!(
            arranged.taskbar_monitor.order,
            [Metric::Disk, Metric::CpuPower, Metric::Cpu]
        );
        assert_eq!(newer.taskbar_monitor.style, TaskbarStyle::Labels);
        assert!(newer.taskbar_monitor.display.is_empty());
        let styled: Settings =
            serde_json::from_str(r#"{"taskbarMonitor": {"style": "figures"}}"#).unwrap();
        assert_eq!(styled.taskbar_monitor.style, TaskbarStyle::Figures);
        let capsules: Settings =
            serde_json::from_str(r#"{"taskbarMonitor": {"style": "capsules"}}"#).unwrap();
        assert_eq!(capsules.taskbar_monitor.style, TaskbarStyle::Capsules);
        let unknown: Settings =
            serde_json::from_str(r#"{"taskbarMonitor": {"style": "neon"}}"#).unwrap();
        assert_eq!(unknown.taskbar_monitor.style, TaskbarStyle::Labels);
        // The desktop monitor's look: a panel of cards in a list unless
        // saved otherwise; names it does not know read as those.
        assert_eq!(
            (
                newer.desktop_monitor.layout,
                newer.desktop_monitor.style,
                newer.desktop_monitor.grouping
            ),
            (DeskLayout::Panel, DeskStyle::List, DeskGrouping::Cards)
        );
        let look: Settings = serde_json::from_str(
            r#"{"desktopMonitor": {"layout": "strip", "style": "gauges", "grouping": "rules"}}"#,
        )
        .unwrap();
        assert_eq!(
            (
                look.desktop_monitor.layout,
                look.desktop_monitor.style,
                look.desktop_monitor.grouping
            ),
            (DeskLayout::Strip, DeskStyle::Gauges, DeskGrouping::Rules)
        );
        let strange: Settings = serde_json::from_str(
            r#"{"desktopMonitor": {"layout": "ring", "style": "3d", "grouping": "boxes"}}"#,
        )
        .unwrap();
        assert_eq!(strange.desktop_monitor, DesktopMonitor::default());
        assert_eq!(
            serde_json::to_value(&look.desktop_monitor).unwrap()["grouping"],
            "rules"
        );
        // A monitor that does not read at all keeps the other settings.
        let mut edited: Settings = serde_json::from_str(
            r#"{"desktopMonitor": {"opacity": "half"}, "taskbarMonitor": 3, "closeToTray": false}"#,
        )
        .unwrap();
        edited.migrate();
        assert_eq!(edited.desktop_monitor, DesktopMonitor::default());
        assert_eq!(edited.taskbar_monitor, TaskbarMonitor::default());
        assert!(!edited.close_to_tray);
        // An opacity on offer nowhere reads as opaque.
        let mut odd: Settings =
            serde_json::from_str(r#"{"desktopMonitor": {"opacity": 55, "locked": true}}"#).unwrap();
        odd.migrate();
        assert_eq!(odd.desktop_monitor.opacity, 100);
        assert!(odd.desktop_monitor.locked);
        // Everything round-trips.
        let saved = Settings {
            desktop_monitor: DesktopMonitor {
                enabled: true,
                metrics: Metric::ALL.map(Reading::from).to_vec(),
                order: vec![Metric::Disk, Metric::DiskTemperature, Metric::Cpu],
                layout: DeskLayout::Strip,
                style: DeskStyle::Curves,
                grouping: DeskGrouping::Stripes,
                topmost: false,
                locked: true,
                click_through: true,
                opacity: 70,
                position: Some([10, 20]),
            },
            taskbar_monitor: TaskbarMonitor {
                enabled: true,
                metrics: vec![
                    Reading::from(Metric::GpuTemperature),
                    Reading::parse(
                        "diskTemperature@disk:nvme|Samsung SSD 990 PRO 2TB|2000398934016#1",
                    )
                    .unwrap(),
                ],
                order: vec![Metric::Network, Metric::Memory],
                place: TaskbarPlace::Start,
                style: TaskbarStyle::Curves,
                display: r"\\?\DISPLAY#ACM0001#5&1&UID4353#{e6f07b5f-ee97-4a90-b076-33f57bf4eaa7}"
                    .into(),
            },
            ..Settings::default()
        };
        let read: Settings = serde_json::from_str(&serde_json::to_string(&saved).unwrap()).unwrap();
        assert_eq!(read.desktop_monitor, saved.desktop_monitor);
        assert_eq!(read.taskbar_monitor, saved.taskbar_monitor);
    }

    #[test]
    fn readings_name_a_device_only_where_a_metric_reads_one() {
        let keyed = Reading::parse("network@nic:Intel(R) Wi-Fi 6E AX211|WLAN").unwrap();
        assert_eq!(keyed.metric, Metric::Network);
        assert_eq!(
            keyed.device.as_deref(),
            Some("nic:Intel(R) Wi-Fi 6E AX211|WLAN")
        );
        assert_eq!(keyed.text(), "network@nic:Intel(R) Wi-Fi 6E AX211|WLAN");
        // A key may hold the separator itself: only the first one counts.
        assert_eq!(
            Reading::parse("gpu@gpu:A@B").unwrap().device.as_deref(),
            Some("gpu:A@B")
        );
        // The CPU and the rest read no device; an empty one is none.
        assert_eq!(Reading::parse("cpu@x"), Some(Reading::from(Metric::Cpu)));
        assert_eq!(Reading::parse("disk@"), Some(Reading::from(Metric::Disk)));
        assert_eq!(Reading::parse("fps@x"), None);
        // Saved lists keep a device's readings apart from the default's.
        let saved: Settings = serde_json::from_str(
            r#"{"taskbarMonitor": {"metrics": ["disk", "disk@disk:sata|WD|4", "disk", "cpu@gpu:x", 7]}}"#,
        )
        .unwrap();
        assert_eq!(
            saved.taskbar_monitor.metrics,
            [
                Reading::from(Metric::Disk),
                Reading::parse("disk@disk:sata|WD|4").unwrap(),
                Reading::from(Metric::Cpu),
            ]
        );
    }

    #[test]
    fn a_hotkey_that_is_not_text_keeps_the_other_settings() {
        let settings: Settings =
            serde_json::from_str(r#"{"hotkey": 5, "closeToTray": false}"#).unwrap();
        assert_eq!(settings.hotkey, DEFAULT_HOTKEY);
        assert!(!settings.close_to_tray);
    }

    #[test]
    fn background_intervals_default_to_5_s_and_drop_odd_values() {
        // Settings from before these existed.
        let mut old: Settings = serde_json::from_str(r#"{"closeToTray": true}"#).unwrap();
        old.migrate();
        assert_eq!((old.minimized_interval, old.tray_interval), (5000, 5000));
        let mut edited: Settings =
            serde_json::from_str(r#"{"minimizedInterval": 3000, "trayInterval": 30000}"#).unwrap();
        edited.migrate();
        assert_eq!(
            (edited.minimized_interval, edited.tray_interval),
            (5000, 30_000)
        );
    }

    #[test]
    fn what_changed_shows_by_default_and_old_settings_have_seen_no_version() {
        let old: Settings =
            serde_json::from_str(r#"{"closeToTray": true, "onboarding": 1}"#).unwrap();
        assert!(old.whats_new);
        assert_eq!(old.seen_version, "");
        let saved: Settings =
            serde_json::from_str(r#"{"whatsNew": false, "seenVersion": "0.2.0"}"#).unwrap();
        assert!(!saved.whats_new);
        assert_eq!(saved.seen_version, "0.2.0");
    }

    #[test]
    fn hardware_acceleration_is_on_until_turned_off() {
        // Settings saved before the choice existed.
        let old: Settings = serde_json::from_str(r#"{"closeToTray": true}"#).unwrap();
        assert!(old.hardware_acceleration);
        let saved = Settings {
            hardware_acceleration: false,
            ..Settings::default()
        };
        let read: Settings = serde_json::from_str(&serde_json::to_string(&saved).unwrap()).unwrap();
        assert!(!read.hardware_acceleration);
    }

    fn scratch() -> PathBuf {
        let id = unsafe { windows::Win32::System::Com::CoCreateGuid() }.unwrap();
        let directory = std::env::temp_dir().join(format!("argus-settings-{id:?}"));
        std::fs::create_dir(&directory).unwrap();
        directory
    }

    #[test]
    fn saved_settings_come_back_and_leave_no_temporary_file() {
        let directory = scratch();
        let path = directory.join("settings.json");
        let store = SettingsStore::load(path.clone());
        store.update(|s| s.onboarding = 1).unwrap();
        store
            .update(|s| s.language = Language::Locale(Locale::EnUs))
            .unwrap();
        let again = SettingsStore::load(path).get();
        assert_eq!(again.onboarding, 1);
        assert_eq!(again.language, Language::Locale(Locale::EnUs));
        let names: Vec<String> = std::fs::read_dir(&directory)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(names, vec!["settings.json"]);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_damaged_file_reads_as_defaults_and_is_kept_aside() {
        let directory = scratch();
        let path = directory.join("settings.json");
        let cut_off = b"{\"onboarding\": 1, \"lang";
        for damaged in [&b""[..], &[0; 300][..], &cut_off[..]] {
            std::fs::write(&path, damaged).unwrap();
            let store = SettingsStore::load(path.clone());
            assert_eq!(store.get().onboarding, 0);
            assert!(!path.exists());
            assert_eq!(
                std::fs::read(directory.join("settings.json.bad")).unwrap(),
                damaged
            );
            // The next change starts a new file and leaves the damaged one be.
            store.update(|s| s.onboarding = 1).unwrap();
            assert_eq!(SettingsStore::load(path.clone()).get().onboarding, 1);
            std::fs::remove_file(&path).unwrap();
        }
        std::fs::remove_dir_all(directory).unwrap();
    }
}
