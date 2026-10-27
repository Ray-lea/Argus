//! Semantic application roles backed by the active GPUI theme.
use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme, Icon, Theme, ThemeMode};
use gpui_kit::*;
#[derive(Clone, Copy)]
pub struct Palette {
    pub ink: Hsla,
    pub muted: Hsla,
    pub faint: Hsla,
    pub background: Hsla,
    pub layer: Hsla,
    pub layer_strong: Hsla,
    pub stroke: Hsla,
    pub control: Hsla,
    pub hover: Hsla,
    pub accent: Hsla,
    pub cpu: Hsla,
    pub memory: Hsla,
    pub io: Hsla,
    pub network: Hsla,
    pub gpu: Hsla,
    pub battery: Hsla,
    pub danger: Hsla,
    pub heat: Hsla,
}
impl Palette {
    pub fn current(cx: &App) -> Self {
        let theme = cx.theme();
        let colors = &theme.colors;
        Self {
            ink: colors.foreground,
            muted: colors.muted_foreground,
            faint: parse_color(
                design()[if theme.mode == ThemeMode::Dark {
                    "dark"
                } else {
                    "light"
                }]["ink-3"]
                    .as_str()
                    .unwrap(),
            ),
            background: if crate::platform::supports_backdrop() {
                colors.background.opacity(0.)
            } else {
                colors.background
            },
            layer: colors.group_box,
            layer_strong: colors.popover,
            stroke: colors.border,
            control: colors.secondary,
            hover: colors.list_hover,
            accent: colors.primary,
            cpu: colors.chart_1,
            memory: colors.chart_2,
            io: colors.chart_3,
            network: colors.chart_4,
            gpu: colors.chart_5,
            battery: colors.chart_bullish,
            danger: colors.danger,
            heat: parse_color(&format!(
                "rgb({})",
                design()[if theme.mode == ThemeMode::Dark {
                    "dark"
                } else {
                    "light"
                }]["heat-rgb"]
                    .as_str()
                    .expect("heat color")
            )),
        }
    }
}
pub fn apply(mode: ThemeMode, window: &mut Window, cx: &mut App) {
    Theme::change(mode, Some(window), cx);
    let design = design();
    let colors = &design[if mode == ThemeMode::Dark {
        "dark"
    } else {
        "light"
    }];
    let color = |key: &str| {
        parse_color(
            colors[key]
                .as_str()
                .or_else(|| design["light"][key].as_str())
                .expect("theme color"),
        )
    };
    let theme = Theme::global_mut(cx);
    let mut tokens = theme.semantic_tokens();
    tokens.colors.background = color("window-bg");
    tokens.colors.foreground = color("ink");
    tokens.colors.surface = color("flyout");
    tokens.colors.surface_foreground = color("ink");
    tokens.colors.primary = color("accent");
    if let Some(accent) = argus_core::theme::accent_colors() {
        tokens.colors.primary = parse_color(if mode == ThemeMode::Dark {
            &accent.light2
        } else {
            &accent.dark1
        });
    }
    tokens.colors.primary_foreground = color("on-accent");
    tokens.colors.secondary = color("control");
    tokens.colors.secondary_foreground = color("ink");
    tokens.colors.muted = color("layer");
    tokens.colors.muted_foreground = color("ink-2");
    tokens.colors.accent = color("hover");
    tokens.colors.accent_foreground = color("ink");
    tokens.colors.destructive = color("danger");
    tokens.colors.destructive_foreground = color("on-danger");
    tokens.colors.border = color("stroke");
    tokens.colors.input = color("control");
    tokens.colors.ring = color("accent");
    tokens.typography.md.size = px(design["fontSize"].as_f64().expect("theme font size") as f32);
    if cfg!(target_os = "windows") {
        tokens.typography.sans = design["fonts"]["windows"][0]
            .as_str()
            .expect("system font")
            .to_owned()
            .into();
    }
    theme.apply_semantic_tokens(&tokens);
    theme.colors.group_box = color("layer");
    theme.colors.list_hover = color("hover");
    theme.colors.table = color("layer");
    theme.colors.table_head = color("layer").opacity(0.);
    theme.colors.table_head_foreground = color("ink-2");
    theme.colors.table_hover = color("hover");
    theme.colors.table_row_border = color("stroke").opacity(0.);
    theme.colors.tab_foreground = color("ink-2");
    theme.colors.tab_active_foreground = color("ink");
    theme.colors.tab_active = color("control-checked");
    theme.colors.tab_bar = color("control");
    theme.colors.tab_bar_segmented = color("window-bg").blend(color("control"));
    theme.colors.switch = color("ink-3");
    theme.colors.switch_thumb = color("window-bg");
    theme.colors.table_even = color("layer");
    theme.colors.title_bar = color("window-bg").opacity(0.);
    theme.colors.title_bar_border = color("stroke").opacity(0.);
    theme.colors.chart_1 = color("cpu");
    theme.colors.chart_2 = color("mem");
    theme.colors.chart_3 = color("io");
    theme.colors.chart_4 = color("net");
    theme.colors.chart_5 = color("gpu");
    theme.colors.chart_bullish = color("bat");
    theme.tokens = theme.colors.into();
    Theme::sync_base(cx);
}
pub fn install_surface(cx: &mut App) {
    gpui_kit::base::Root::register_plugin(cx, |_, _| ProductSurface);
}
struct ProductSurface;
impl Render for ProductSurface {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}
impl gpui_kit::base::RootPlugin for ProductSurface {
    fn prepare(&mut self, window: &mut Window, _: &mut Context<Self>) {
        window.set_rem_size(dimension("unit"));
    }
    fn style(&self, surface: &mut Stateful<Div>, _: &mut Window, _: &mut App) {
        if crate::platform::supports_backdrop() {
            use gpui_kit::gpui::Refineable;
            surface
                .style()
                .refine(&StyleRefinement::default().bg(gpui_kit::gpui::transparent_black()));
        }
    }
}
pub fn design() -> &'static serde_json::Value {
    static DESIGN: std::sync::OnceLock<serde_json::Value> = std::sync::OnceLock::new();
    DESIGN.get_or_init(|| {
        serde_json::from_str(include_str!("../../../resources/theme.json")).expect("product theme")
    })
}
pub fn dimension(key: &str) -> Pixels {
    px(design()["layout"][key].as_f64().expect("layout token") as f32)
}
pub fn heat_alpha(field: &str, value: f64) -> f32 {
    let heat = &design()["heat"];
    let kind = match field {
        "cpu" | "gpu" => "percent",
        "mem" => "memory",
        _ => "rate",
    };
    let rule = &heat[kind];
    let minimum = rule["minimum"].as_f64().expect("heat threshold");
    if value < minimum || !value.is_finite() {
        return 0.;
    }
    let level = if kind == "rate" {
        (value / minimum + 1.).log10() / rule["logScale"].as_f64().unwrap()
    } else {
        value / rule["full"].as_f64().unwrap()
    };
    (level.clamp(0., 1.) * heat["opacity"].as_f64().unwrap()) as f32
}
pub fn transition() -> gpui_kit::base::motion::Transition {
    gpui_kit::base::motion::Transition::new(motion_duration())
}
pub fn motion_duration() -> std::time::Duration {
    let milliseconds = design()["light"]["t-med"]
        .as_str()
        .expect("motion duration")
        .trim_end_matches("ms")
        .parse()
        .expect("motion milliseconds");
    std::time::Duration::from_millis(milliseconds)
}
pub(super) fn parse_color(css: &str) -> Hsla {
    if let Some(hex) = css.strip_prefix('#') {
        return rgb(u32::from_str_radix(hex, 16).expect("theme hex color")).into();
    }
    let components = css
        .strip_prefix("rgb(")
        .and_then(|s| s.strip_suffix(')'))
        .expect("theme RGB color")
        .split_whitespace()
        .filter(|p| *p != "/")
        .map(|p| p.parse::<f32>().expect("theme color component"))
        .collect::<Vec<_>>();
    Rgba {
        r: components[0] / 255.,
        g: components[1] / 255.,
        b: components[2] / 255.,
        a: components.get(3).copied().unwrap_or(1.),
    }
    .into()
}
pub fn icon(name: &str, color: Hsla) -> Icon {
    // Names follow the original icon set; a few native-only names alias to it.
    let original = match name {
        "CpuIcon" => "ChipIcon",
        "DownloadIcon" => "UpdateIcon",
        "RefreshIcon" => "RestartIcon",
        "LanguageIcon" => "GlobeIcon",
        "MotionIcon" => "SparkleIcon",
        "AppIcon" => "GenericAppIcon",
        "DataIcon" => "StorageIcon",
        "RulesIcon" => "BoltIcon",
        "ExternalLinkIcon" => "ExternalIcon",
        "CloseIcon" => "DismissIcon",
        name => name,
    };
    let mut file = String::new();
    for (index, c) in original
        .strip_suffix("Icon")
        .unwrap_or(original)
        .chars()
        .enumerate()
    {
        if c.is_ascii_uppercase() && index > 0 {
            file.push('-');
        }
        file.push(c.to_ascii_lowercase());
    }
    if let Some(path) = super::assets::glyph(&file) {
        return Icon::empty().path(path).size_4().text_color(color);
    }
    let icon = match name {
        "ProcessesIcon" => IconName::LayoutList,
        "PulseIcon" => IconName::Activity,
        "ServicesIcon" => IconName::Settings2,
        "PowerIcon" => IconName::Power,
        "DownloadIcon" => IconName::Download,
        "GlobeIcon" => IconName::Network,
        "HistoryIcon" => IconName::Clock,
        "SettingsIcon" => IconName::Settings,
        "MenuIcon" => IconName::Menu,
        "AppsIcon" => IconName::PanelsTopLeft,
        "TreeIcon" => IconName::ListTree,
        "ListIcon" => IconName::List,
        "PauseIcon" => IconName::Pause,
        "PlayIcon" => IconName::Play,
        "LockIcon" => IconName::Lock,
        "PanelIcon" => IconName::PanelRight,
        "RunTaskIcon" => IconName::SquareTerminal,
        "ChevronDown" => IconName::ChevronDown,
        "ChevronRight" => IconName::ChevronRight,
        "ChevronUp" => IconName::ChevronUp,
        "MinimizeIcon" => IconName::Minus,
        "MaximizeIcon" => IconName::Square,
        "DismissIcon" => IconName::X,
        "CpuIcon" => IconName::Cpu,
        "MemoryIcon" => IconName::MemoryStick,
        "DiskIcon" => IconName::HardDrive,
        "GpuIcon" => IconName::Monitor,
        "BatteryIcon" => IconName::Battery,
        "RefreshIcon" => IconName::RefreshCw,
        "LanguageIcon" => IconName::Globe,
        "KeyboardIcon" => IconName::Keyboard,
        "ShieldIcon" => IconName::Shield,
        "MotionIcon" => IconName::Sparkles,
        "InfoIcon" => IconName::Info,
        "CopyIcon" => IconName::Copy,
        "EyeIcon" => IconName::Eye,
        "EyeOffIcon" => IconName::EyeOff,
        "AppIcon" => IconName::AppWindow,
        "ThemeIcon" => IconName::SunMoon,
        "DataIcon" => IconName::Database,
        "RulesIcon" => IconName::Zap,
        "SearchIcon" => IconName::Search,
        "ExternalLinkIcon" => IconName::ExternalLink,
        "FolderIcon" => IconName::FolderOpen,
        _ => IconName::Circle,
    };
    Icon::new(icon).size_4().text_color(color)
}
pub fn label(key: &str) -> String {
    argus_core::i18n::text(key)
}
pub fn count(value: f64) -> String {
    let raw = format!("{:.0}", value);
    let mut output = String::new();
    let len = raw.len();
    for (i, c) in raw.chars().enumerate() {
        if i > 0 && (len - i).is_multiple_of(3) {
            output.push(',');
        }
        output.push(c);
    }
    output
}
pub fn cpu(value: f64) -> String {
    if value < 0.05 {
        "0".into()
    } else if value >= 10. {
        format!("{value:.0}")
    } else {
        format!("{value:.1}")
    }
}
pub fn memory(value: f64) -> String {
    let gb = value / 1073741824.;
    if gb >= 10. {
        format!("{gb:.1} GB")
    } else if gb >= 1. {
        format!("{gb:.2} GB")
    } else {
        format!("{:.1} MB", value / 1048576.)
    }
}
pub fn bytes(value: f64, rate: bool) -> String {
    let suffix = if rate { "/s" } else { "" };
    let mut amount = value;
    let mut unit = 0;
    while amount >= 1024. && unit < 4 {
        amount /= 1024.;
        unit += 1;
    }
    let names = ["B", "KB", "MB", "GB", "TB"];
    if value == 0. {
        return "0".into();
    }
    if unit <= 1 {
        format!("{amount:.0} {}{suffix}", names[unit])
    } else {
        format!("{amount:.1} {}{suffix}", names[unit])
    }
}
pub fn network(value: f64, bits: bool) -> String {
    if !bits {
        return bytes(value, true);
    }
    let mut amount = value * 8.;
    let mut unit = 0;
    let units = ["b/s", "Kbps", "Mbps", "Gbps", "Tbps"];
    while amount >= 1000. && unit + 1 < units.len() {
        amount /= 1000.;
        unit += 1;
    }
    format!("{amount:.1} {}", units[unit])
}
pub fn timestamp(ms: i64) -> String {
    let Ok(value) = time::OffsetDateTime::from_unix_timestamp(ms.div_euclid(1000)) else {
        return label("format.missing");
    };
    let offset = time::UtcOffset::current_local_offset().unwrap_or(time::UtcOffset::UTC);
    let value = value.to_offset(offset);
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        value.year(),
        u8::from(value.month()),
        value.day(),
        value.hour(),
        value.minute(),
        value.second()
    )
}
pub fn uptime(seconds: f64) -> String {
    let s = seconds as u64;
    let days = s / 86400;
    let hours = s / 3600 % 24;
    let minutes = s / 60 % 60;
    if days > 0 {
        argus_core::tr!("format.uptime.days", days = days, hours = hours)
    } else {
        format!("{hours}:{minutes:02}:{:02}", s % 60)
    }
}
