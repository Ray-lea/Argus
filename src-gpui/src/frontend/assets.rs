//! Product icons extend the component bundle without copying its SVG source.
use gpui_kit::{AssetSource, SharedString};
use std::borrow::Cow;
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
