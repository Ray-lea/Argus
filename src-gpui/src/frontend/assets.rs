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
pub struct Assets;
impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        if path == "argus/logo.svg" {
            return Ok(Some(Cow::Borrowed(include_bytes!(
                "../../../resources/logo.svg"
            ))));
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
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}
