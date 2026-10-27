//! Product icons extend the component bundle without copying its SVG source.
use gpui_kit::{AssetSource, SharedString};
use std::borrow::Cow;
gpui_kit::assets::icon_assets!(
    ProductIcons,
    [
        LayoutList,
        Activity,
        Settings2,
        Power,
        Download,
        Network,
        Clock,
        Settings,
        Menu,
        PanelsTopLeft,
        ListTree,
        List,
        Pause,
        Play,
        Lock,
        PanelRight,
        SquareTerminal,
        ChevronDown,
        ChevronRight,
        ChevronUp,
        Minus,
        Square,
        X,
        Cpu,
        MemoryStick,
        HardDrive,
        Monitor,
        Battery,
        RefreshCw,
        Globe,
        Keyboard,
        Shield,
        Sparkles,
        Info,
        Copy,
        Eye,
        EyeOff,
        AppWindow,
        SunMoon,
        Database,
        Zap,
        Search,
        Circle
    ]
);
/// The original 16px line glyphs, drawn to match Fluent's regular weight.
macro_rules! glyphs {
    ($($name:literal),* $(,)?) => {
        const GLYPHS: &[(&str, &str, &[u8])] = &[$((
            $name,
            concat!("argus/glyphs/", $name, ".svg"),
            include_bytes!(concat!("../../../resources/glyphs/", $name, ".svg")),
        )),*];
    };
}
glyphs!(
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
    "eye-off",
    "eye",
    "filter",
    "folder",
    "gauge",
    "generic-app-fill",
    "generic-app",
    "globe",
    "go-to",
    "gpu",
    "history",
    "info",
    "kernel-fill",
    "kernel",
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
);
/// Asset path of an original glyph, by its kebab-case name.
pub fn glyph(name: &str) -> Option<&'static str> {
    GLYPHS
        .iter()
        .find(|(glyph, _, _)| *glyph == name)
        .map(|(_, path, _)| *path)
}
pub struct Assets;
impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        if path == "argus/logo.svg" {
            return Ok(Some(Cow::Borrowed(include_bytes!(
                "../../../resources/logo.svg"
            ))));
        }
        if let Some((_, _, bytes)) = GLYPHS.iter().find(|(_, glyph, _)| *glyph == path) {
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
            GLYPHS
                .iter()
                .filter(|(_, glyph, _)| glyph.starts_with(path))
                .map(|(_, glyph, _)| SharedString::from(*glyph)),
        );
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}
