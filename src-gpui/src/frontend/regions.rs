//! Parts of the desktop drawn as cached views of their own. The desktop keeps
//! all state; a region renders a part of it. Any desktop notification renders
//! every region again (a notified view refreshes the cached views inside it),
//! but an animation frame or a scroll requested while a region renders redraws
//! only that region: its ancestors render their own trees and replay the other
//! regions. Live charts belong to the region that built them.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum RegionKind {
    /// The current page, below the navigation (entering pages move it whole).
    Page,
    /// The process details pane.
    Details,
    /// The resource cards above the process list.
    Vitals,
    /// The title bar row: logo, page controls, search and actions.
    Toolbar,
    /// The page tabs (top) or the side navigation.
    Navigation,
}

pub(super) struct Region {
    desktop: WeakEntity<Desktop>,
    kind: RegionKind,
}

impl Render for Region {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let kind = self.kind;
        self.desktop
            .update(cx, |desktop, cx| desktop.render_region(kind, window, cx))
            .unwrap_or_else(|_| div().into_any_element())
    }
}

#[derive(Default)]
pub(super) struct Regions {
    views: HashMap<RegionKind, Entity<Region>>,
    /// The chart ids each region registered when it last rendered.
    charts: HashMap<RegionKind, HashSet<String>>,
    /// Renders of each region: chart continuity counts these.
    renders: HashMap<RegionKind, u64>,
    /// The regions the page placed inside it when it last rendered.
    placed: HashSet<RegionKind>,
}

impl Desktop {
    /// The region as a cached view laid out at `style`.
    pub(super) fn region(
        &mut self,
        kind: RegionKind,
        style: StyleRefinement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.regions.placed.insert(kind);
        let desktop = cx.entity().downgrade();
        self.regions
            .views
            .entry(kind)
            .or_insert_with(|| cx.new(|_| Region { desktop, kind }))
            .clone()
            .cached(style)
            .into_any_element()
    }

    /// Draws one region again on the next frame; the desktop around it renders
    /// its own tree and replays the other regions.
    pub(super) fn refresh_region(&self, kind: RegionKind, cx: &mut Context<Self>) {
        if let Some(view) = self.regions.views.get(&kind) {
            view.update(cx, |_, cx| cx.notify());
        }
    }

    fn render_region(
        &mut self,
        kind: RegionKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.forget_region_charts(kind);
        let before = self.active_charts.borrow().clone();
        let renders = self.regions.renders.entry(kind).or_default();
        *renders = renders.wrapping_add(1);
        let render_count = std::mem::replace(&mut self.render_count, *renders);
        let element = match kind {
            RegionKind::Page => {
                self.regions.placed.clear();
                // The flex column the page used to fill directly.
                let element = div()
                    .relative()
                    .size_full()
                    .min_h_0()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(self.page_content(window, cx))
                    .into_any_element();
                // Regions of the page it no longer shows take their charts along,
                // and their charts start afresh when they come back (as after a
                // break in the desktop's renders).
                for nested in [RegionKind::Details, RegionKind::Vitals] {
                    if !self.regions.placed.contains(&nested) {
                        self.forget_region_charts(nested);
                        let renders = self.regions.renders.entry(nested).or_default();
                        *renders = renders.wrapping_add(2);
                    }
                }
                element
            }
            RegionKind::Details => self.details_view(window, cx),
            RegionKind::Vitals => div()
                .size_full()
                .flex()
                .flex_col()
                .child(self.vitals(window, cx))
                .into_any_element(),
            // Each in the flex container it used to fill: a column for the
            // toolbar and the tabs, the body row for the side navigation.
            RegionKind::Toolbar => div()
                .size_full()
                .flex()
                .flex_col()
                .child(self.toolbar(window, cx))
                .into_any_element(),
            RegionKind::Navigation => div()
                .size_full()
                .flex()
                .when(self.nav_top, |d| d.flex_col())
                .child(self.navigation(self.nav_top, window, cx))
                .into_any_element(),
        };
        self.render_count = render_count;
        let added = self
            .active_charts
            .borrow()
            .difference(&before)
            .cloned()
            .collect();
        if kind == RegionKind::Details {
            self.pane_motion.borrow_mut().charts.clone_from(&added);
        }
        self.regions.charts.insert(kind, added);
        self.schedule_chart_tick(window, cx);
        element
    }

    /// Renders the current page again when called (after an animation that
    /// moved parts of it with live transforms), or the whole desktop when the
    /// page is not a region of its own.
    pub(super) fn render_page_later(&self, cx: &Context<Self>) -> impl FnOnce(&mut App) + 'static {
        let page = self
            .regions
            .views
            .get(&RegionKind::Page)
            .map(|page| page.downgrade());
        let desktop = cx.entity().downgrade();
        move |cx| {
            let rendered = page
                .and_then(|page| page.update(cx, |_, cx| cx.notify()).ok())
                .is_some();
            if !rendered {
                let _ = desktop.update(cx, |_, cx| cx.notify());
            }
        }
    }

    /// Stops drawing the charts a region built (it renders them again, or is gone).
    fn forget_region_charts(&mut self, kind: RegionKind) {
        if kind == RegionKind::Details {
            self.pane_motion.borrow_mut().charts.clear();
        }
        if let Some(ids) = self.regions.charts.remove(&kind) {
            let mut active = self.active_charts.borrow_mut();
            for id in ids {
                active.remove(&id);
            }
        }
    }
}
