//! Product icons come from the Lucide catalog bundled with the component
//! library: only the ones named here are embedded, restyled to the
//! interface's stroke as they are loaded.
use gpui_kit::assets::IconName;
use gpui_kit::{App, AssetSource, Hsla, RenderImage, Rgba, SharedString};
use std::{borrow::Cow, cell::RefCell, collections::HashMap, sync::Arc};

/// The interface's icon names and the Lucide icon each one draws. The one
/// table gives the lookup, the embedded set and, for the tests, the names:
/// an icon named here cannot be missing from the binary.
macro_rules! product_icons {
    ($($($name:literal)|+ => $icon:ident,)*) => {
        gpui_kit::assets::icon_assets!(ProductIcons, [$($icon,)* Circle]);

        /// The Lucide icon for a product icon name; unknown names show a circle.
        pub fn icon_name(name: &str) -> IconName {
            match name {
                $($($name)|+ => IconName::$icon,)*
                _ => IconName::Circle,
            }
        }

        #[cfg(test)]
        const NAMES: &[(&str, IconName)] = &[$($(($name, IconName::$icon),)+)*];
    };
}
product_icons! {
    "ProcessesIcon" => LayoutList,
    "PulseIcon" => Activity,
    "ServicesIcon" => Puzzle,
    "PowerIcon" => Power,
    "DownloadIcon" | "UpdateIcon" => Download,
    "GlobeIcon" | "LanguageIcon" => Globe,
    "HistoryIcon" | "DataIcon" => RotateCcwClock,
    "SettingsIcon" => Settings,
    "MenuIcon" => Menu,
    "AppsIcon" => LayoutGrid,
    "TreeIcon" => ListTree,
    "ExpandAllIcon" => ChevronsUpDown,
    "CollapseAllIcon" => ChevronsDownUp,
    "ListIcon" => List,
    "PauseIcon" => Pause,
    "PlayIcon" => Play,
    "LockIcon" => Lock,
    "PanelIcon" => PanelRight,
    "PanelAutoIcon" => PanelRightDashed,
    "RunTaskIcon" => SquareTerminal,
    "ChevronDown" => ChevronDown,
    "ChevronRight" => ChevronRight,
    "ChevronUp" => ChevronUp,
    "MinimizeIcon" => Minimize2,
    "MaximizeIcon" | "StopIcon" => Square,
    "DismissIcon" | "CloseIcon" => X,
    "CpuIcon" | "ChipIcon" => Cpu,
    "KernelIcon" => Microchip,
    "MemoryIcon" => MemoryStick,
    "DiskIcon" => HardDrive,
    "GpuIcon" => Gpu,
    "NetworkIcon" => Network,
    "BatteryIcon" => BatteryMedium,
    "ComputerIcon" => Monitor,
    "RefreshIcon" | "RestartIcon" => RotateCw,
    "ResetIcon" => RotateCcw,
    "KeyboardIcon" => Keyboard,
    "ShieldIcon" => Shield,
    "MotionIcon" | "SparkleIcon" => Sparkles,
    "InfoIcon" => Info,
    "TipIcon" => Lightbulb,
    "CopyIcon" => Copy,
    "EyeIcon" => Eye,
    "EyeOffIcon" => EyeOff,
    "AppIcon" | "GenericAppIcon" => AppWindow,
    "ThemeIcon" => Contrast,
    "StorageIcon" => Database,
    "RulesIcon" | "BoltIcon" => Zap,
    "SearchIcon" => Search,
    "ExternalLinkIcon" | "ExternalIcon" => ExternalLink,
    "FolderIcon" => Folder,
    "FilterIcon" => Funnel,
    "LeafIcon" => Leaf,
    "NoteIcon" => StickyNote,
    "NotesIcon" => ScrollText,
    "UserIcon" => User,
    "UsersIcon" => Users,
    "AccountsIcon" => UserCog,
    "SignOutIcon" => LogOut,
    "PinIcon" => Pin,
    "UnpinIcon" => PinOff,
    "TrashIcon" => Trash,
    "GoToIcon" | "ArrowRightIcon" => ArrowRight,
    "EndIcon" => CircleX,
    "BranchEndIcon" => ListX,
    "GaugeIcon" => Gauge,
    "TrayIcon" => PanelBottom,
    "TimerIcon" => Timer,
    "TransferIcon" => ArrowUpDown,
    // The filled one is the same outline, filled as it is loaded (`icon_path`).
    "StarIcon" | "StarFilledIcon" => Star,
    "ChatIcon" => MessageSquare,
    "DocumentIcon" => FileText,
    "NavSideIcon" => PanelLeft,
    "NavTopIcon" => PanelTop,
    "WarningIcon" => TriangleAlert,
    "CheckIcon" => Check,
    "PointerIcon" => MousePointer2,
    "ColumnsIcon" => Columns3,
    "FontSizeIcon" => ALargeSmall,
    "InterfaceScaleIcon" => ZoomIn,
    "GripIcon" => GripVertical,
    "FitWidthIcon" => MoveHorizontal,
    "OrderIcon" => ListOrdered,
    "SortAscendingIcon" => ArrowUpNarrowWide,
    "SortDescendingIcon" => ArrowDownWideNarrow,
    // The catalog keeps GitHub's solid mark under this name, not a line icon.
    "GithubIcon" => Github,
}

/// Lucide draws 2 px strokes on its 24 px grid, 1.33 px at the interface's
/// 16 px. Product icons keep the interface's 1.25 px there: the catalog's
/// SVG is served with this stroke width instead.
const STROKE_WIDTH: &str = "1.875";
/// Where the restyled icons are served: stroked, and filled as well.
const STROKED: &str = "argus/stroked/";
const FILLED: &str = "argus/filled/";

/// The asset path of a product icon in the interface's stroke. A name ending
/// in `FilledIcon` fills the outline too.
pub fn icon_path(name: &str) -> SharedString {
    let prefix = if name.ends_with("FilledIcon") {
        FILLED
    } else {
        STROKED
    };
    format!("{prefix}{}", icon_name(name).path()).into()
}

/// A catalog icon in the interface's stroke, its outline filled if asked.
/// Icons without a stroke (the GitHub mark) are served as they are.
fn restyle(svg: &[u8], filled: bool) -> Vec<u8> {
    let mut svg = String::from_utf8_lossy(svg).replacen(
        "stroke-width=\"2\"",
        &format!("stroke-width=\"{STROKE_WIDTH}\""),
        1,
    );
    if filled {
        svg = svg.replacen("fill=\"none\"", "fill=\"currentColor\"", 1);
    }
    svg.into_bytes()
}

/// One arrow of a number box's spin button in `color`, for a 15 × 9 px half
/// of the button at `scale`: a triangle with rounded corners, as Chromium
/// draws it there. It is a colour image rather than a tinted icon: icons get
/// the text contrast boost, which draws these 7 px arrows visibly heavier
/// than the interface's plain coverage.
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

pub struct Assets;
impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        if path == "argus/logo.svg" {
            return Ok(Some(Cow::Borrowed(include_bytes!(
                "../../../resources/logo.svg"
            ))));
        }
        for (prefix, filled) in [(STROKED, false), (FILLED, true)] {
            if let Some(icon) = path.strip_prefix(prefix) {
                return Ok(ProductIcons
                    .load(icon)?
                    .map(|svg| Cow::Owned(restyle(&svg, filled))));
            }
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
        for prefix in [STROKED, FILLED] {
            paths.extend(
                ProductIcons
                    .list("")?
                    .into_iter()
                    .map(|icon| format!("{prefix}{icon}"))
                    .filter(|icon| icon.starts_with(path))
                    .map(SharedString::from),
            );
        }
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}

#[cfg(test)]
mod tests {
    use super::{Assets, FILLED, NAMES, STROKE_WIDTH, STROKED, icon_name, icon_path};
    use gpui_kit::AssetSource;
    use gpui_kit::assets::IconName;

    fn svg(path: &str) -> String {
        let bytes = Assets
            .load(path)
            .expect("asset source")
            .unwrap_or_else(|| panic!("{path} is not embedded"));
        String::from_utf8_lossy(&bytes).into_owned()
    }

    #[test]
    fn every_product_icon_is_embedded_in_the_interface_stroke() {
        assert!(NAMES.len() > 80);
        for (name, icon) in NAMES {
            assert_eq!(icon_name(name), *icon);
            let path = icon_path(name);
            let svg = svg(&path);
            assert!(svg.contains("<svg"), "{name}");
            // The catalog's own stroke is gone wherever the icon has one.
            assert!(!svg.contains("stroke-width=\"2\""), "{name}");
            if *icon != IconName::Github {
                assert!(
                    svg.contains(&format!("stroke-width=\"{STROKE_WIDTH}\"")),
                    "{name}"
                );
            }
        }
    }

    #[test]
    fn a_filled_name_fills_the_same_outline() {
        let outline = svg(&icon_path("StarIcon"));
        let filled = svg(&icon_path("StarFilledIcon"));
        assert!(icon_path("StarIcon").starts_with(STROKED));
        assert!(icon_path("StarFilledIcon").starts_with(FILLED));
        assert!(outline.contains("fill=\"none\""));
        assert!(!filled.contains("fill=\"none\""));
        assert_eq!(
            outline.replacen("fill=\"none\"", "fill=\"currentColor\"", 1),
            filled
        );
    }

    #[test]
    fn unknown_names_show_a_circle_and_the_listing_names_what_loads() {
        assert_eq!(icon_name("NoSuchIcon"), IconName::Circle);
        assert!(svg(&icon_path("NoSuchIcon")).contains("<circle"));
        let listed = Assets.list(STROKED).expect("listing");
        assert!(listed.contains(&icon_path("ProcessesIcon")));
        for path in listed {
            assert!(Assets.load(&path).expect("asset source").is_some());
        }
    }
}
