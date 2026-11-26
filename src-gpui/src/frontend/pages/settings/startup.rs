//! Startup and background: the page a start opens on, and the page the
//! window opens on from the tray (shown on the Components page).
use super::*;

impl Desktop {
    /// The page a start opens on (`startPage`): any page but Settings, named
    /// as in the navigation; the card wears the chosen page's icon.
    pub(super) fn start_page_setting(&self, cx: &mut Context<Self>) -> AnyElement {
        let current = start_page(&self.prefs.get("startPage", Value::Null));
        let pages = start_pages().collect::<Vec<_>>();
        let glyph = pages
            .iter()
            .find(|(page, _)| *page == current)
            .map_or(PAGES[0].1, |(_, glyph)| glyph);
        let select = self.select(
            "settings-start-page",
            label("settings.startup.startPage.title"),
            pages
                .iter()
                .map(|(page, _)| label(&format!("shell.page.{page}")))
                .collect(),
            pages.iter().position(|(page, _)| *page == current),
            false,
            move |this, index, window, cx| {
                this.preference("startPage", json!(pages[index].0), window, cx);
            },
            cx,
        );
        self.setting_text(
            "startPage",
            glyph,
            "settings.startup.startPage.title",
            "settings.startup.startPage.desc",
            vec![select],
        )
    }

    pub(in crate::frontend) fn tray_page_setting(&self, cx: &mut Context<Self>) -> AnyElement {
        let modes = ["current", "default"];
        let current = self.prefs.get::<String>("trayPage", "current".into());
        let select = self.select(
            "settings-tray-page",
            label("settings.startup.trayPage.title"),
            modes
                .iter()
                .map(|mode| label(&format!("settings.startup.trayPage.{mode}")))
                .collect(),
            Some(modes.iter().position(|mode| *mode == current).unwrap_or(0)),
            false,
            move |this, index, window, cx| {
                this.preference("trayPage", json!(modes[index]), window, cx);
            },
            cx,
        );
        self.setting_text(
            "trayPage",
            "AppIcon",
            "settings.startup.trayPage.title",
            "settings.startup.trayPage.desc",
            vec![select],
        )
    }
}
