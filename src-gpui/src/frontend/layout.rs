//! Application layout uses the native component rhythm and theme roles.
use super::*;
use gpui_kit::base::ElementExt;
use gpui_kit::component::ActiveTheme;
impl Desktop {
    pub(in crate::frontend) fn set_theme(
        &mut self,
        dark: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        theme::apply(
            if dark {
                ThemeMode::Dark
            } else {
                ThemeMode::Light
            },
            window,
            cx,
        );
        self.palette = Palette::current(cx);
        for (table, frame) in self.tables.values() {
            table.update(cx, |_, cx| cx.notify());
            frame.update(cx, |_, cx| cx.notify());
        }
        cx.notify();
    }
    pub(in crate::frontend) fn button(
        &self,
        id: impl Into<SharedString>,
        text: impl Into<SharedString>,
        glyph: Option<&str>,
        action: Action,
        selected: bool,
        cx: &Context<Self>,
    ) -> Button {
        let text = text.into();
        let p = self.palette;
        Button::new(id.into())
            .text_size(cx.theme().font_size)
            .min_h(theme::dimension("controlHeight"))
            .flex_shrink_0()
            .px_3()
            .py_1()
            .gap_2()
            .rounded(cx.theme().radius)
            .text_color(p.ink)
            .when(selected, |b| b.bg(p.control))
            .hover(move |s| s.bg(p.hover).text_color(p.ink))
            .accessibility_label(text.clone())
            .when_some(glyph, |b, name| {
                b.child(icon(name, if selected { p.accent } else { p.muted }))
            })
            .when(!text.is_empty(), |b| b.child(text))
            .on_click(cx.listener(move |this, _, window, cx| this.act(action.clone(), window, cx)))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
    }
    pub(in crate::frontend) fn navigation(
        &self,
        top: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if top {
            let p = self.palette;
            let selected = PAGES
                .iter()
                .position(|page| page.0 == self.page)
                .unwrap_or(0);
            let regions = *self.navigation_regions.borrow();
            let marker_width = theme::dimension("navigationIndicatorWidth");
            let marker_x = gpui_kit::base::motion::transition(
                "page-navigation-indicator",
                regions[selected + 1].center().x - regions[0].left() - marker_width / 2.,
                theme::transition(),
                window,
                cx,
            );
            let owner = cx.entity().downgrade();
            let tabs = gpui_kit::base::Tabs::new("page-navigation")
                .relative()
                .flex()
                .gap_1()
                .h(theme::dimension("navigationHeight"))
                .on_prepaint(track_region(
                    0,
                    self.navigation_regions.clone(),
                    owner.clone(),
                ))
                .children(PAGES.iter().enumerate().map(|(index, (page, _))| {
                    let name = label(&format!("shell.page.{page}"));
                    gpui_kit::base::Tab::new(("page", index))
                        .selected(index == selected)
                        .set_position(index + 1, PAGES.len())
                        .accessibility_label(name.clone())
                        .px_2()
                        .h_full()
                        .cursor_pointer()
                        .text_color(self.palette.muted)
                        .rounded_md()
                        .hover(move |style| style.bg(p.hover).text_color(p.ink))
                        .when(index == selected, |tab| {
                            tab.text_color(self.palette.ink)
                                .font_weight(FontWeight::SEMIBOLD)
                        })
                        .child(name)
                        .on_prepaint(track_region(
                            index + 1,
                            self.navigation_regions.clone(),
                            owner.clone(),
                        ))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.act(Action::Page((*page).into()), window, cx);
                        }))
                }))
                .when(regions[selected + 1].size.width > px(0.), |tabs| {
                    tabs.child(
                        div()
                            .absolute()
                            .bottom_0()
                            .left(marker_x)
                            .w(marker_width)
                            .h(theme::dimension("navigationIndicatorHeight"))
                            .rounded_full()
                            .bg(self.palette.accent),
                    )
                });
            return div()
                .px_4()
                .pb(px(6.))
                .flex_shrink_0()
                .child(tabs)
                .into_any_element();
        }
        let mut nav = div()
            .flex()
            .flex_shrink_0()
            .gap_1()
            .when(top, |d| d.px_3().pb_2().flex_wrap())
            .when(!top, |d| {
                d.flex_col()
                    .px_1()
                    .pb_2()
                    .when(self.nav_expanded, |d| d.w_48())
                    .when(!self.nav_expanded, |d| d.w_12())
            });
        if !top {
            nav = nav.child(self.button(
                "nav-toggle",
                "",
                Some("MenuIcon"),
                Action::Preference("navExpanded".into(), json!(!self.nav_expanded)),
                false,
                cx,
            ));
        }
        for (page, glyph) in PAGES {
            if !top && page == "settings" {
                nav = nav.child(div().flex_1());
            }
            let name = label(&format!("shell.page.{page}"));
            nav = nav.child(
                self.button(
                    format!("nav-{page}"),
                    if top || self.nav_expanded {
                        name.clone()
                    } else {
                        String::new()
                    },
                    (!top).then_some(glyph),
                    Action::Page(page.into()),
                    page == self.page,
                    cx,
                )
                .relative()
                .justify_start()
                .when(page == self.page, |b| {
                    b.child(
                        div()
                            .absolute()
                            .left_0()
                            .w_1()
                            .h_4()
                            .rounded_full()
                            .bg(self.palette.accent),
                    )
                })
                .role(Role::Tab)
                .accessibility_label(name),
            );
        }
        nav.into_any_element()
    }
    pub(in crate::frontend) fn toolbar(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.palette;
        let mut controls = div()
            .flex()
            .items_center()
            .gap_2()
            .flex_shrink_0()
            .occlude();
        if self.page == "processes" {
            controls = controls.child(
                self.segments(
                    "process-view",
                    [
                        ("apps", "AppsIcon"),
                        ("tree", "TreeIcon"),
                        ("list", "ListIcon"),
                    ]
                    .into_iter()
                    .map(|(mode, glyph)| {
                        (
                            if window.viewport_size().width >= window.rem_size() * 64. {
                                label(&format!("shell.viewMode.{mode}.label"))
                            } else {
                                String::new()
                            },
                            Some(glyph),
                            Action::View(mode.into()),
                            self.view == mode,
                        )
                    })
                    .collect(),
                    cx,
                ),
            );
        } else {
            controls = controls.child(self.page_controls(window, cx));
        }
        let sampled = matches!(self.page.as_str(), "processes" | "performance");
        let mut trailing = div()
            .flex()
            .items_center()
            .gap_1()
            .flex_shrink_0()
            .occlude()
            .child(
                self.button(
                    "task",
                    if window.viewport_size().width > px(1100.) {
                        label("tasks.title")
                    } else {
                        String::new()
                    },
                    Some("RunTaskIcon"),
                    Action::Dialog("task".into(), json!({})),
                    false,
                    cx,
                )
                .border_1()
                .border_color(p.stroke)
                .bg(p.control)
                .accessibility_label(label("tasks.title")),
            );

        let intervals = [500, 1000, 2000, 5000]
            .into_iter()
            .map(|ms| {
                MenuItem::new(
                    tr!("shell.interval.option", seconds = ms as f64 / 1000.),
                    Action::Interval(ms),
                )
            })
            .collect();
        if sampled {
            trailing = trailing.child(
                self.button(
                    "interval",
                    tr!(
                        "shell.interval.option",
                        seconds = self.interval as f64 / 1000.
                    ),
                    Some("ChevronDown"),
                    Action::Popup(intervals),
                    false,
                    cx,
                )
                .border_1()
                .border_color(p.stroke)
                .bg(p.control),
            );
        }
        for (id, key, glyph, action) in [
            (
                "pause",
                if self.paused {
                    "shell.resume"
                } else {
                    "shell.pause"
                },
                if self.paused { "PlayIcon" } else { "PauseIcon" },
                Action::Pause,
            ),
            (
                "lock",
                "shell.fileLock.label",
                "LockIcon",
                Action::Dialog("filelock".into(), json!({})),
            ),
            (
                "details",
                "shell.details.label",
                "PanelIcon",
                Action::Details,
            ),
        ] {
            if (id == "details" && self.page != "processes") || (id == "pause" && !sampled) {
                continue;
            }
            trailing = trailing.child(
                self.button(
                    id,
                    String::new(),
                    Some(glyph),
                    action,
                    (id == "pause" && self.paused) || (id == "details" && self.details_open),
                    cx,
                )
                .border_1()
                .border_color(p.stroke)
                .bg(p.control)
                .accessibility_label(label(key))
                .tooltip(move |window, cx| {
                    gpui_kit::component::tooltip::Tooltip::new(label(key)).build(window, cx)
                }),
            );
        }
        if let Some(updates) = self
            .data
            .get("updates")
            .filter(|s| matches!(text(s, "state"), "ready" | "available" | "downloading"))
        {
            let ver = text(updates, "version");
            trailing = trailing.child(
                self.button(
                    "update-status",
                    ver.to_owned(),
                    Some("DownloadIcon"),
                    Action::Dialog("update".into(), json!({})),
                    false,
                    cx,
                )
                .border_1()
                .border_color(p.accent.opacity(0.45))
                .bg(p.accent.opacity(0.14))
                .text_color(p.accent)
                .accessibility_label(label("settings.update.check")),
            );
        }
        let owner = cx.entity().downgrade();
        let start = div()
            .flex()
            .items_center()
            .gap_2()
            .flex_shrink_0()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_color(p.ink)
                    .child(img("argus/logo.svg").size_5())
                    .child(argus_app::config::app_title()),
            )
            .child(controls)
            .on_prepaint(track_region(0, self.toolbar_regions.clone(), owner.clone()));
        let trailing = trailing.on_prepaint(track_region(1, self.toolbar_regions.clone(), owner));
        let mut toolbar = div()
            .relative()
            .w_full()
            .h(theme::dimension("toolbarHeight"))
            .flex_shrink_0()
            .child(
                TitleBar::new()
                    .h(theme::dimension("toolbarHeight"))
                    .bg(p.background)
                    .border_0()
                    .child(
                        div()
                            .size_full()
                            .flex()
                            .items_center()
                            .justify_between()
                            .px_1()
                            .gap_2()
                            .child(start)
                            .child(div().flex_1())
                            .child(trailing),
                    ),
            );
        if matches!(
            self.page.as_str(),
            "processes" | "services" | "startup" | "network"
        ) {
            let regions = *self.toolbar_regions.borrow();
            let left = regions[0].right() + px(8.);
            let right = regions[1].left() - px(8.);
            if right > left {
                let width = (right - left).min(theme::dimension("searchWidth"));
                let centered = (window.viewport_size().width - width) / 2.;
                let x = centered.max(left).min(right - width);
                toolbar = toolbar.child(
                    div()
                        .absolute()
                        .left(x)
                        .top(
                            (theme::dimension("toolbarHeight") - theme::dimension("controlHeight"))
                                / 2.,
                        )
                        .h(theme::dimension("controlHeight"))
                        .w(width)
                        .flex()
                        .items_center()
                        .occlude()
                        .child(
                            Input::new(&self.search)
                                .cleanable(true)
                                .prefix(icon("SearchIcon", p.muted)),
                        ),
                );
            }
        }
        toolbar.into_any_element()
    }
    pub(in crate::frontend) fn statusbar(&self, cx: &Context<Self>) -> AnyElement {
        let p = self.palette;
        let sys = &self.snapshot["system"];
        let mut left = div()
            .flex()
            .gap_4()
            .items_center()
            .flex_1()
            .overflow_hidden();
        for (key, value) in [
            (
                "shell.status.threads",
                theme::count(number(sys, "threadCount")),
            ),
            (
                "shell.status.handles",
                theme::count(number(sys, "handleCount")),
            ),
            ("shell.status.cached", theme::memory(number(sys, "cached"))),
            (
                "shell.status.pagedPool",
                theme::memory(number(sys, "kernelPaged")),
            ),
            (
                "shell.status.uptime",
                theme::uptime(number(sys, "uptimeMs") / 1000.),
            ),
        ] {
            left = left.child(
                div()
                    .flex()
                    .gap_1()
                    .whitespace_nowrap()
                    .child(label(key))
                    .child(div().text_color(p.muted).child(value)),
            );
        }
        div()
            .h(theme::dimension("statusHeight"))
            .px_4()
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap_4()
            .text_xs()
            .text_color(p.faint)
            .child(left)
            .child(div().text_color(p.accent).child(if self.paused {
                tr!("shell.status.paused")
            } else if self.held_order {
                tr!("shell.status.orderHeld")
            } else {
                tr!(
                    "shell.status.interval",
                    seconds = self.interval as f64 / 1000.
                )
            }))
            .child(
                self.button(
                    "privilege-badge",
                    label(if boolean(&self.data["info"], "elevated") {
                        "shell.elevation.admin"
                    } else {
                        "shell.elevation.standard"
                    }),
                    None,
                    Action::Page("settings".into()),
                    false,
                    cx,
                )
                .min_h_0()
                .h_5()
                .py_0()
                .border_1()
                .border_color(p.stroke)
                .rounded_full()
                .text_xs(),
            )
            .into_any_element()
    }
    pub(in crate::frontend) fn mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some((kind, origin, start)) = &self.drag {
            if kind == "details" {
                let em = f32::from(window.rem_size());
                self.details_width = (*start - f32::from(event.position.x - origin.x))
                    .clamp(em * 12., f32::from(window.viewport_size().width) * 0.45);
                cx.notify();
            } else if kind == "perf-sidebar" {
                self.perf_sidebar_width =
                    (*start + f32::from(event.position.x - origin.x)).clamp(200., 440.);
                cx.notify();
            }
        }
    }
}
impl Render for Desktop {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let render_started = (self.options.test
            && std::env::var_os("ARGUS_GPUI_VIEW_TRACE").is_some())
        .then(Instant::now);
        self.render_count = self.render_count.wrapping_add(1);
        self.active_charts.borrow_mut().clear();
        let p = self.palette;
        let toolbar = self.toolbar(window, cx);
        let content = self.page_content(window, cx);
        let footer = self.statusbar(cx);
        let body = div()
            .flex_1()
            .min_h_0()
            .flex()
            .when(!self.nav_top, |d| {
                d.child(self.navigation(false, window, cx))
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .child(div().flex_1().min_h_0().child(self.enter(
                        ("page-enter", self.page_epoch),
                        content,
                        cx,
                    )))
                    .child(footer),
            );
        let overlays = self.render_overlays(window, cx);
        self.schedule_chart_tick(window, cx);
        let result = div()
            .id("argus-root")
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(p.background)
            .text_color(p.ink)
            .font_family(cx.theme().font_family.clone())
            .text_size(cx.theme().font_size)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .on_modifiers_changed(cx.listener(Self::modifiers_changed))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                let paths = paths
                    .paths()
                    .iter()
                    .map(|path| path.to_string_lossy().into_owned())
                    .collect::<Vec<_>>();
                this.open_dialog("filelock", json!({"paths":paths}), window, cx);
                if let Some(input) = this.forms.get("value") {
                    input.update(cx, |state, cx| {
                        state.set_value(paths.join("\n"), window, cx)
                    });
                }
                this.request(
                    "find_lockers",
                    json!({"paths":paths}),
                    Some("lockers"),
                    window,
                    cx,
                );
            }))
            .on_mouse_move(cx.listener(Self::mouse_move))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if let Some((kind, _, _)) = this.drag.take() {
                        if kind == "details" {
                            this.prefs.set("detailsWidth", this.details_width);
                        } else if kind == "perf-sidebar" {
                            this.prefs.set("perfSideWidth", this.perf_sidebar_width);
                        }
                    }
                    cx.notify();
                }),
            )
            .child(toolbar)
            .when(self.nav_top, |d| d.child(self.navigation(true, window, cx)))
            .child(body)
            .children(overlays);
        if let Some(render_started) = render_started {
            println!(
                "{}",
                json!({"event":"ui-render-cost","micros":render_started.elapsed().as_micros(),"page":self.page})
            );
        }
        result
    }
}

fn track_region<const N: usize>(
    index: usize,
    regions: Rc<RefCell<[Bounds<Pixels>; N]>>,
    owner: WeakEntity<Desktop>,
) -> impl Fn(Bounds<Pixels>, &mut Window, &mut App) + 'static {
    move |bounds, window, cx| {
        if regions.borrow()[index] == bounds {
            return;
        }
        regions.borrow_mut()[index] = bounds;
        let owner = owner.clone();
        window.defer(cx, move |_, cx| {
            let _ = owner.update(cx, |_, cx| cx.notify());
        });
    }
}
