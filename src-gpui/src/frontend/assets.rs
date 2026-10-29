//! Product icons extend the component bundle without copying its SVG source.
use gpui_kit::{App, AssetSource, Hsla, RenderImage, Rgba, SharedString};
use std::{borrow::Cow, cell::RefCell, collections::HashMap, sync::Arc};
gpui_kit::assets::icon_assets!(
    ProductIcons,
    [
        Activity,
        AppWindow,
        ArrowLeftRight,
        ArrowRight,
        Battery,
        Check,
        ChevronDown,
        ChevronRight,
        ChevronUp,
        Circle,
        CircleX,
        Clock,
        Copy,
        Cpu,
        Database,
        Download,
        EthernetPort,
        ExternalLink,
        Eye,
        EyeOff,
        FileText,
        FolderOpen,
        Funnel,
        Gauge,
        Globe,
        Gpu,
        HardDrive,
        Info,
        Keyboard,
        LayoutGrid,
        LayoutList,
        Leaf,
        List,
        ListTree,
        ListX,
        Lock,
        MemoryStick,
        Menu,
        MessageCircle,
        Microchip,
        Minus,
        Monitor,
        MousePointer2,
        Network,
        NotebookText,
        PanelBottom,
        PanelLeft,
        PanelRight,
        PanelTop,
        Pause,
        Pin,
        PinOff,
        Play,
        Power,
        RotateCw,
        Search,
        Settings,
        Settings2,
        Shield,
        Sparkles,
        Square,
        SquareArrowOutUpRight,
        SquareTerminal,
        Star,
        StickyNote,
        SunMoon,
        Timer,
        Trash,
        TriangleAlert,
        X,
        Zap
    ]
);
/// The original interface's own 16 px line icons (its `components/icons.tsx`),
/// one SVG each under `resources/ui-icons`, served as `argus/icons/<name>.svg`.
macro_rules! ui_icons {
    ($($name:literal),* $(,)?) => {
        const UI_ICONS: &[(&str, &[u8])] = &[$((
            $name,
            include_bytes!(concat!("../../../resources/ui-icons/", $name, ".svg")),
        )),*];
    };
}
ui_icons![
    "apps",
    "arrow-right",
    "battery",
    "bolt",
    "branch-end",
    "chat",
    "check",
    "chevron-down",
    "chevron-right",
    "chevron-up",
    "chip",
    "computer",
    "copy",
    "disk",
    "dismiss",
    "document",
    "end",
    "external",
    "eye",
    "eye-off",
    "filter",
    "folder",
    "font-size",
    "gauge",
    "generic-app",
    "generic-app-fill",
    "globe",
    "go-to",
    "gpu",
    "history",
    "info",
    "interface-scale",
    "kernel",
    "kernel-fill",
    "keyboard",
    "leaf",
    "list",
    "lock",
    "memory",
    "menu",
    "minimize",
    "nav-side",
    "nav-top",
    "network",
    "note",
    "notes",
    "panel",
    "pause",
    "pin",
    "play",
    "pointer",
    "power",
    "processes",
    "pulse",
    "restart",
    "run-task",
    "search",
    "services",
    "settings",
    "shield",
    "sparkle",
    "star",
    "stop",
    "storage",
    "theme",
    "timer",
    "transfer",
    "trash",
    "tray",
    "tree",
    "unpin",
    "update",
    "warning",
];
const UI_ICON_PREFIX: &str = "argus/icons/";

/// One arrow of a number box's spin button in `color`, for a 15 × 9 px half
/// of the button at `scale`: a triangle with rounded corners, as Chromium
/// draws it there. It is a colour image rather than a tinted icon: icons get
/// the text contrast boost, which draws these 7 px arrows visibly heavier
/// than the original's plain coverage.
pub fn spin_arrow(up: bool, color: Hsla, scale: f32, cx: &App) -> Option<Arc<RenderImage>> {
    thread_local! {
        static ARROWS: RefCell<HashMap<(bool, u32, u32), Arc<RenderImage>>> =
            RefCell::new(HashMap::new());
    }
    let rgba = Rgba::from(color);
    let [r, g, b] = [rgba.r, rgba.g, rgba.b].map(|c| (c * 255.).round() as u8);
    let key = (up, u32::from_be_bytes([0, r, g, b]), scale.to_bits());
    if let Some(image) = ARROWS.with_borrow(|arrows| arrows.get(&key).cloned()) {
        return Some(image);
    }
    let source: &[u8] = if up {
        include_bytes!("../../../resources/spin-up.svg")
    } else {
        include_bytes!("../../../resources/spin-down.svg")
    };
    let svg = String::from_utf8_lossy(source).replace("#000", &format!("#{r:02x}{g:02x}{b:02x}"));
    let image = cx
        .svg_renderer()
        .render_single_frame(svg.as_bytes(), scale)
        .ok()?;
    ARROWS.with_borrow_mut(|arrows| arrows.insert(key, image.clone()));
    Some(image)
}

/// The asset path of the original icon for a product icon name, if it has one.
pub fn ui_icon(name: &str) -> Option<SharedString> {
    // Names the desktop uses for an icon the original calls otherwise.
    let name = match name {
        "AppIcon" => "GenericAppIcon",
        "CloseIcon" => "DismissIcon",
        "CpuIcon" => "ChipIcon",
        "DataIcon" => "HistoryIcon",
        "ExternalLinkIcon" => "ExternalIcon",
        "LanguageIcon" => "GlobeIcon",
        "MotionIcon" => "SparkleIcon",
        "RefreshIcon" => "RestartIcon",
        "RulesIcon" => "BoltIcon",
        name => name,
    };
    let mut file = String::new();
    let stem = name.strip_suffix("Icon").unwrap_or(name);
    for (index, c) in stem.char_indices() {
        if c.is_ascii_uppercase() && index > 0 {
            file.push('-');
        }
        file.push(c.to_ascii_lowercase());
    }
    UI_ICONS
        .iter()
        .any(|(icon, _)| *icon == file)
        .then(|| format!("{UI_ICON_PREFIX}{file}.svg").into())
}

pub struct Assets;
impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        if path == "argus/logo.svg" {
            return Ok(Some(Cow::Borrowed(include_bytes!(
                "../../../resources/logo.svg"
            ))));
        }
        if let Some(file) = path
            .strip_prefix(UI_ICON_PREFIX)
            .and_then(|name| name.strip_suffix(".svg"))
            && let Some((_, bytes)) = UI_ICONS.iter().find(|(icon, _)| *icon == file)
        {
            return Ok(Some(Cow::Borrowed(bytes)));
        }
        if let Some(bytes) = ProductIcons.load(path)? {
            return Ok(Some(bytes));
        }
        gpui_kit::assets::Assets.load(path)
    }
    fn list(&self, path: &str) -> anyhow::Result<Vec<SharedString>> {
        let mut paths = ProductIcons.list(path)?;
        paths.extend(gpui_kit::assets::Assets.list(path)?);
        if "argus/logo.svg".starts_with(path) {
            paths.push("argus/logo.svg".into());
        }
        paths.extend(
            UI_ICONS
                .iter()
                .map(|(icon, _)| format!("{UI_ICON_PREFIX}{icon}.svg"))
                .filter(|icon| icon.starts_with(path))
                .map(SharedString::from),
        );
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}
