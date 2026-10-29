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
    pub stroke_strong: Hsla,
    pub tip: Hsla,
    pub control: Hsla,
    pub hover: Hsla,
    pub accent: Hsla,
    pub cpu: Hsla,
    pub cpu_e: Hsla,
    pub memory: Hsla,
    pub io: Hsla,
    pub network: Hsla,
    pub gpu: Hsla,
    pub battery: Hsla,
    pub danger: Hsla,
    pub heat: Hsla,
    /// Icons standing in for a missing process icon.
    pub glyph: Hsla,
    pub hover_strong: Hsla,
    /// Fluent roles used by controls, dialogs and notices.
    pub flyout: Hsla,
    /// The Fluent control face (`--control`), unmixed by the component theme.
    pub face: Hsla,
    /// Fluent controls as the original drew them: translucent faces
    /// (`--control`, `--control-hover`, `--control-checked`) over whatever
    /// is behind, a 1 px stroke, and a stronger bottom edge (`control_edge`).
    pub control_face: Hsla,
    pub control_hover: Hsla,
    pub control_checked: Hsla,
    /// Laid over the 1 px stroke along a control's bottom, the two make the
    /// original's `--stroke-strong` bottom border (GPUI borders have one colour).
    pub control_edge: Hsla,
    pub press: Hsla,
    pub on_accent: Hsla,
    pub on_danger: Hsla,
    pub dark: bool,
}
/// `name` over a card (`--layer`) over the window, as one opaque colour.
fn on_card(name: &str, dark: bool) -> Hsla {
    let color = token("window-bg", dark)
        .blend(token("layer", dark))
        .blend(token(name, dark));
    Hsla { a: 1., ..color }
}
/// What, laid over `under`, gives `over` in its place: both are the same
/// colour at different strengths (the strokes are white or black).
fn remainder(under: Hsla, over: Hsla) -> Hsla {
    Hsla {
        a: 1. - (1. - over.a) / (1. - under.a),
        ..over
    }
}
/// A colour token of the active mode, falling back to the light value.
pub fn token(name: &str, dark: bool) -> Hsla {
    let design = design();
    parse_color(
        design[if dark { "dark" } else { "light" }][name]
            .as_str()
            .or_else(|| design["light"][name].as_str())
            .expect("theme token"),
    )
}
/// A product token without a component-library role, for the active mode.
pub fn role(cx: &App, key: &str) -> Hsla {
    token(key, cx.theme().mode == ThemeMode::Dark)
}
/// What a cached palette depends on: the mode, the accent and the base colours.
type PaletteKey = (ThemeMode, Hsla, Hsla, Hsla);
impl Palette {
    /// The palette of the active theme. Built once per theme and accent: table
    /// cells ask for it on every frame, and building it parses theme tokens.
    pub fn current(cx: &App) -> Self {
        thread_local! {
            static CACHE: std::cell::Cell<Option<(PaletteKey, Palette)>> =
                const { std::cell::Cell::new(None) };
        }
        let theme = cx.theme();
        let key = (
            theme.mode,
            theme.colors.primary,
            theme.colors.foreground,
            theme.colors.background,
        );
        if let Some((cached, palette)) = CACHE.get()
            && cached == key
        {
            return palette;
        }
        let palette = Self::build(cx);
        CACHE.set(Some((key, palette)));
        palette
    }
    fn build(cx: &App) -> Self {
        let theme = cx.theme();
        let colors = &theme.colors;
        let dark = theme.mode == ThemeMode::Dark;
        Self {
            glyph: token("glyph-stroke", dark),
            hover_strong: token("hover-strong", dark),
            stroke_strong: token("stroke-strong", dark),
            flyout: token("flyout", dark),
            // Opaque equivalents of the translucent control colours over a card:
            // stacked translucent fills darken on the transparent window surface.
            face: on_card("control", dark),
            control_face: token("control", dark),
            control_hover: token("control-hover", dark),
            control_checked: token("control-checked", dark),
            control_edge: remainder(token("stroke", dark), token("stroke-strong", dark)),
            press: token("press", dark),
            on_accent: colors.primary_foreground,
            on_danger: token("on-danger", dark),
            dark,
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
            tip: token("tip", dark),
            control: colors.secondary,
            hover: colors.list_hover,
            accent: colors.primary,
            cpu: colors.chart_1,
            cpu_e: token("cpu-e", dark),
            memory: colors.chart_2,
            io: colors.chart_3,
            network: colors.chart_4,
            gpu: colors.chart_5,
            battery: colors.chart_bullish,
            danger: role(cx, "danger"),
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
    let sans = installed_font("sans", cx);
    let theme = Theme::global_mut(cx);
    let mut tokens = theme.semantic_tokens();
    tokens.colors.background = color("window-bg");
    tokens.colors.foreground = color("ink");
    // Opaque: menus paint their shadow under the whole surface, where it would
    // darken a translucent one (the original blurs what is behind instead).
    tokens.colors.surface = Hsla {
        a: 1.,
        ..color("flyout")
    };
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
    tokens.typography.md.size = font(base_font_size());
    tokens.typography.sans = sans;
    theme.apply_semantic_tokens(&tokens);
    theme.colors.group_box = color("layer");
    theme.colors.list_hover = color("hover");
    // The workspace card paints the layer; the table itself stays clear.
    theme.colors.table = color("layer").opacity(0.);
    theme.colors.table_head = color("layer").opacity(0.);
    theme.colors.table_head_foreground = color("ink-2");
    theme.colors.table_hover = color("hover");
    theme.colors.table_row_border = color("stroke").opacity(0.);
    // Selected rows: the accent at 15%, with the bar drawn by the row itself.
    theme.colors.table_active = theme.colors.primary.opacity(0.15);
    theme.colors.table_active_border = theme.colors.primary.opacity(0.);
    theme.list.active_highlight = true;
    // The library's danger role colours the caption close button: Windows red, white glyph.
    theme.colors.danger = rgb(0xc42b1c).into();
    theme.colors.danger_hover = rgb(0xc42b1c).into();
    theme.colors.danger_active = Hsla::from(rgb(0xc42b1c)).opacity(0.9);
    theme.colors.danger_foreground = gpui_kit::gpui::white();
    theme.colors.tab_foreground = color("ink-2");
    theme.colors.tab_active_foreground = color("ink");
    theme.colors.tab_active = color("control-checked");
    theme.colors.tab_bar = color("control");
    theme.colors.tab_bar_segmented = color("window-bg").blend(color("control"));
    theme.colors.switch = color("ink-3");
    theme.colors.switch_thumb = color("window-bg");
    theme.colors.table_even = color("layer").opacity(0.);
    theme.colors.title_bar = color("window-bg").opacity(0.);
    theme.colors.title_bar_border = color("stroke").opacity(0.);
    theme.colors.chart_1 = color("cpu");
    theme.colors.chart_2 = color("mem");
    theme.colors.chart_3 = color("io");
    theme.colors.chart_4 = color("net");
    theme.colors.chart_5 = color("gpu");
    theme.colors.chart_bullish = color("bat");
    // Dialogs dim the window with their own fading layer (`dialogs::Scrim`);
    // the component backdrop only takes the clicks outside them.
    theme.colors.overlay = color("scrim").opacity(0.);
    // Dialogs, menus and notices use the 8 px radius of the original surfaces.
    theme.radius_lg = px(8.);
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
    fn style(&self, surface: &mut Stateful<Div>, _: &mut Window, cx: &mut App) {
        // Characters the interface family lacks (Chinese above all) take the
        // rest of the theme's list, as the original's CSS font stack did. Left
        // to DirectWrite, an English system locale renders Chinese in the
        // Japanese Yu Gothic UI.
        surface.text_style().font_fallbacks = Some(font_fallbacks(cx));
        // The page, dialogs and notices inherit the original body's line height.
        surface.text_style().line_height = Some(line_height());
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
/// Large figures and titles use the display optical size where the system has it.
pub fn display_font(cx: &App) -> SharedString {
    installed_font("display", cx)
}
/// Addresses, ports, commands and code use the monospaced family.
pub fn mono_font(cx: &App) -> SharedString {
    installed_font("mono", cx)
}
/// The interface families after the one in use, for characters it lacks.
fn font_fallbacks(cx: &App) -> FontFallbacks {
    static FALLBACKS: std::sync::OnceLock<FontFallbacks> = std::sync::OnceLock::new();
    FALLBACKS
        .get_or_init(|| {
            let sans = installed_font("sans", cx);
            FontFallbacks::from_fonts(
                design()["fonts"]["sans"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(serde_json::Value::as_str)
                    .skip_while(|name| *name != sans.as_ref())
                    .skip(1)
                    .map(str::to_owned)
                    .collect(),
            )
        })
        .clone()
}
/// The first family of a theme font list that the system has; a missing
/// family would otherwise fall back to a proportional default.
fn installed_font(kind: &str, cx: &App) -> SharedString {
    static SANS: std::sync::OnceLock<SharedString> = std::sync::OnceLock::new();
    static DISPLAY: std::sync::OnceLock<SharedString> = std::sync::OnceLock::new();
    static MONO: std::sync::OnceLock<SharedString> = std::sync::OnceLock::new();
    let cell = match kind {
        "sans" => &SANS,
        "mono" => &MONO,
        _ => &DISPLAY,
    };
    cell.get_or_init(|| {
        let system = cx.text_system();
        design()["fonts"][kind]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(serde_json::Value::as_str)
            .find(|name| {
                let id = system.resolve_font(&gpui_kit::font(name.to_string()));
                system
                    .get_font_for_id(id)
                    .is_some_and(|resolved| resolved.family.as_ref() == *name)
            })
            .map(|name| SharedString::from(name.to_owned()))
            .unwrap_or_else(|| {
                if kind == "mono" {
                    cx.theme().mono_font_family.clone()
                } else {
                    cx.theme().font_family.clone()
                }
            })
    })
    .clone()
}
pub fn dimension(key: &str) -> Pixels {
    px(design()["layout"][key].as_f64().expect("layout token") as f32)
}
thread_local! {
    /// The base font size (`fontSize`) over the theme's, see `font`.
    static FONT_SCALE: std::cell::Cell<f32> = const { std::cell::Cell::new(1.) };
}
/// The theme's base font size, which the interface's text sizes are written for.
pub fn base_font_size() -> f32 {
    design()["fontSize"].as_f64().expect("theme font size") as f32
}
/// The line height text inherits, as a multiple of its size: the original
/// body's `line-height: 1.4`. GPUI's default (about 1.618) made wrapped text,
/// and the boxes it sizes, taller than the original's.
pub fn line_height() -> DefiniteLength {
    relative(design()["lineHeight"].as_f64().expect("theme line height") as f32)
}
/// A text size written for the theme's 13 px base, following the base font
/// size the user chose: the original's `calc(N rem / 13)`. Spacing and
/// control sizes stay in pixels, as there.
pub fn font(size: f32) -> Pixels {
    px(size * FONT_SCALE.get())
}
/// A text size from the layout tokens (figures, titles), as `font`.
pub fn font_dimension(key: &str) -> Pixels {
    font(f32::from(dimension(key)))
}
/// The width of one line of interface text at `size`.
pub fn text_width(text: &str, size: Pixels, window: &Window, cx: &App) -> Pixels {
    let mut font = gpui_kit::font(cx.theme().font_family.clone());
    font.fallbacks = Some(font_fallbacks(cx));
    let run = TextRun {
        len: text.len(),
        font,
        color: Hsla::default(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    window
        .text_system()
        .layout_line(text, size, &[run], None)
        .width
}
/// How much taller a line of `size` px text at `line_height` gets at the
/// chosen font size: what the original's content-sized boxes grew by, for
/// heights written here in pixels (zero at the default size).
pub fn text_growth(size: f32, line_height: f32) -> Pixels {
    (font(size) - px(size)) * line_height
}
/// Sets the base font size `font` scales to; the caller applies the theme
/// again (the component font size follows) and redraws.
pub fn set_font_size(size: f32) {
    FONT_SCALE.set(size / base_font_size());
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
/// The original's slower motion, 250 ms on the Fluent move curve; still under reduced motion.
pub fn transition_slow(cx: &App) -> gpui_kit::base::motion::Transition {
    gpui_kit::base::motion::Transition::new(if cx.reduce_motion() {
        std::time::Duration::ZERO
    } else {
        std::time::Duration::from_millis(250)
    })
    .ease(gpui_kit::base::animation::cubic_bezier(0.33, 0., 0.1, 1.))
}
pub fn transition() -> gpui_kit::base::motion::Transition {
    gpui_kit::base::motion::Transition::new(motion_duration())
}
/// A CSS cubic-bezier timing function sampled at `t`.
pub fn bezier(x1: f32, y1: f32, x2: f32, y2: f32, t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    let curve = |a: f32, b: f32, s: f32| {
        let u = 1. - s;
        3. * u * u * s * a + 3. * u * s * s * b + s * s * s
    };
    // Solve x(s) = t by bisection; the curves used here are monotonic in x.
    let (mut low, mut high) = (0f32, 1f32);
    for _ in 0..24 {
        let mid = (low + high) / 2.;
        if curve(x1, x2, mid) < t {
            low = mid;
        } else {
            high = mid;
        }
    }
    curve(y1, y2, (low + high) / 2.)
}
/// `from` moved toward `to` by `t`, the way CSS animates and `color-mix`es
/// colours: premultiplied sRGB, alpha included. (`Hsla::blend` keeps the
/// first colour's alpha, so it cannot bring a translucent stroke to the accent.)
pub fn mix(from: Hsla, to: Hsla, t: f32) -> Hsla {
    let (a, b) = (Rgba::from(from), Rgba::from(to));
    let t = t.clamp(0., 1.);
    let alpha = a.a + (b.a - a.a) * t;
    if alpha <= 0. {
        return Hsla::transparent_black();
    }
    let channel = |x: f32, y: f32| (x * a.a + (y * b.a - x * a.a) * t) / alpha;
    Rgba {
        r: channel(a.r, b.r),
        g: channel(a.g, b.g),
        b: channel(a.b, b.b),
        a: alpha,
    }
    .into()
}
/// Fluent's decelerating entrance curve.
pub fn ease_out(t: f32) -> f32 {
    bezier(0.1, 0.9, 0.2, 1., t)
}
/// Fluent's accelerating exit curve.
pub fn ease_in(t: f32) -> f32 {
    bezier(0.7, 0., 1., 0.5, t)
}
/// Fluent's curve for things moving between two places.
pub fn ease_move(t: f32) -> f32 {
    bezier(0.33, 0., 0.1, 1., t)
}
/// A transition of `ms` milliseconds on one of the curves above.
pub fn fluent(ms: u64, curve: fn(f32) -> f32) -> gpui_kit::base::motion::Transition {
    gpui_kit::base::motion::Transition::new(std::time::Duration::from_millis(ms)).ease(curve)
}
/// The display face for dialog titles and the About name.
pub const DISPLAY_FONT: &str = "Segoe UI Variable Display";
/// Monospaced text: commands, paths, licences.
pub const MONO_FONT: &str = "Cascadia Mono";
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
/// The component set's icon for a product icon name; unknown names show a circle.
pub fn icon_name(name: &str) -> IconName {
    match name {
        "ProcessesIcon" => IconName::LayoutList,
        "PulseIcon" => IconName::Activity,
        "ServicesIcon" => IconName::Settings2,
        "PowerIcon" => IconName::Power,
        "DownloadIcon" => IconName::Download,
        "UpdateIcon" => IconName::Download,
        "GlobeIcon" => IconName::Globe,
        "HistoryIcon" => IconName::Clock,
        "SettingsIcon" => IconName::Settings,
        "MenuIcon" => IconName::Menu,
        "AppsIcon" => IconName::LayoutGrid,
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
        "CloseIcon" => IconName::X,
        "CpuIcon" => IconName::Cpu,
        "ChipIcon" => IconName::Cpu,
        "KernelIcon" => IconName::Microchip,
        "MemoryIcon" => IconName::MemoryStick,
        "DiskIcon" => IconName::HardDrive,
        "GpuIcon" => IconName::Gpu,
        "NetworkIcon" => IconName::EthernetPort,
        "BatteryIcon" => IconName::Battery,
        "ComputerIcon" => IconName::Monitor,
        "RefreshIcon" => IconName::RotateCw,
        "RestartIcon" => IconName::RotateCw,
        "LanguageIcon" => IconName::Globe,
        "KeyboardIcon" => IconName::Keyboard,
        "ShieldIcon" => IconName::Shield,
        "MotionIcon" => IconName::Sparkles,
        "SparkleIcon" => IconName::Sparkles,
        "InfoIcon" => IconName::Info,
        "CopyIcon" => IconName::Copy,
        "EyeIcon" => IconName::Eye,
        "EyeOffIcon" => IconName::EyeOff,
        "AppIcon" => IconName::AppWindow,
        "GenericAppIcon" => IconName::AppWindow,
        "ThemeIcon" => IconName::SunMoon,
        "DataIcon" => IconName::Database,
        "StorageIcon" => IconName::Database,
        "RulesIcon" => IconName::Zap,
        "BoltIcon" => IconName::Zap,
        "SearchIcon" => IconName::Search,
        "ExternalLinkIcon" => IconName::ExternalLink,
        "ExternalIcon" => IconName::ExternalLink,
        "FolderIcon" => IconName::FolderOpen,
        "FilterIcon" => IconName::Funnel,
        "LeafIcon" => IconName::Leaf,
        "NoteIcon" => IconName::StickyNote,
        "NotesIcon" => IconName::NotebookText,
        "PinIcon" => IconName::Pin,
        "UnpinIcon" => IconName::PinOff,
        "TrashIcon" => IconName::Trash,
        "GoToIcon" => IconName::SquareArrowOutUpRight,
        "EndIcon" => IconName::CircleX,
        "BranchEndIcon" => IconName::ListX,
        "GaugeIcon" => IconName::Gauge,
        "StopIcon" => IconName::Square,
        "TrayIcon" => IconName::PanelBottom,
        "TimerIcon" => IconName::Timer,
        "TransferIcon" => IconName::ArrowLeftRight,
        "StarIcon" => IconName::Star,
        "ChatIcon" => IconName::MessageCircle,
        "DocumentIcon" => IconName::FileText,
        "NavSideIcon" => IconName::PanelLeft,
        "NavTopIcon" => IconName::PanelTop,
        "WarningIcon" => IconName::TriangleAlert,
        "CheckIcon" => IconName::Check,
        "PointerIcon" => IconName::MousePointer2,
        "ArrowRightIcon" => IconName::ArrowRight,
        _ => IconName::Circle,
    }
}
/// The original's own icon for a product icon name; the component set's
/// nearest one (`icon_name`) for the few the original does not have.
pub fn icon(name: &str, color: Hsla) -> Icon {
    glyph(name).text_color(color)
}
/// An icon in the text colour around it (`currentColor`), following hover.
pub fn glyph(name: &str) -> Icon {
    match super::assets::ui_icon(name) {
        Some(path) => Icon::empty().path(path),
        None => Icon::new(icon_name(name)),
    }
    .size_4()
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
    if rate {
        // Rates read in whole kilobytes at least; idle shows a bare 0.
        const KB: f64 = 1024.;
        const MB: f64 = KB * 1024.;
        const GB: f64 = MB * 1024.;
        return if value < 1. {
            "0".into()
        } else if value < MB {
            format!("{} KB/s", (value / KB).round().max(1.))
        } else if value < GB {
            format!("{:.1} MB/s", value / MB)
        } else {
            format!("{:.2} GB/s", value / GB)
        };
    }
    let suffix = "";
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

