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
        let p = self.palette;
        let selected = PAGES
            .iter()
            .position(|page| page.0 == self.page)
            .unwrap_or(0);
        let regions = *self.navigation_regions.borrow();
        let owner = cx.entity().downgrade();
        let hover = p.hover;
        let press = p.press;
        let hover_strong = p.hover_strong;
        let update_waiting = self
            .data
            .get("updates")
            .is_some_and(|s| matches!(text(s, "state"), "ready" | "available"));
        let slide = theme::transition_slow(cx);
        let marker_width = theme::dimension("navigationIndicatorWidth");
        let marker_x = gpui_kit::base::motion::transition(
            "page-navigation-indicator",
            regions[selected + 1].center().x - regions[0].left() - marker_width / 2.,
            slide.clone(),
            window,
            cx,
        );
        let marker_y = gpui_kit::base::motion::transition(
            "page-navigation-pane-indicator",
            regions[selected + 1].center().y - regions[0].top() - px(8.),
            slide,
            window,
            cx,
        );
        let entry = |index: usize, page: &'static str, glyph: &'static str| {
            let name = label(&format!("shell.page.{page}"));
            let chosen = index == selected;
            let title = tr!("shell.pageTitle", page = name.clone(), key = index + 1);
            let has_dot = page == "settings" && update_waiting;
            let dot =
                move || has_dot.then(|| div().absolute().size(px(6.)).rounded_full().bg(p.accent));
            gpui_kit::base::Tab::new(("page", index))
                .selected(chosen)
                .set_position(index + 1, PAGES.len())
                .accessibility_label(name.clone())
                .relative()
                .flex()
                .flex_shrink_0()
                .items_center()
                .rounded(px(4.))
                .px(px(12.))
                .text_color(if chosen { p.ink } else { p.muted })
                .hover(move |s| s.bg(hover).text_color(p.ink))
                .active(move |s| s.bg(press))
                .tooltip(move |window, cx| {
                    gpui_kit::component::tooltip::Tooltip::new(title.clone()).build(window, cx)
                })
                .on_prepaint(track_region(
                    index + 1,
                    self.navigation_regions.clone(),
                    owner.clone(),
                ))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.act(Action::Page(page.into()), window, cx);
                }))
                .when(top, |tab| {
                    // The semibold width is reserved, so choosing a tab never moves the others.
                    tab.h(px(32.)).child(
                        div()
                            .relative()
                            .child(
                                div()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .invisible()
                                    .child(name.clone()),
                            )
                            .child(
                                div()
                                    .absolute()
                                    .inset_0()
                                    .flex()
                                    .justify_center()
                                    .when(chosen, |d| d.font_weight(FontWeight::SEMIBOLD))
                                    .child(name.clone()),
                            )
                            .children(dot().map(|dot| dot.top(px(-1.)).right(px(-7.)))),
                    )
                })
                .when(!top, |tab| {
                    tab.h(px(36.))
                        .justify_start()
                        .gap(px(12.))
                        .when(chosen, |tab| tab.bg(hover_strong))
                        .child(
                            div()
                                .relative()
                                .flex_shrink_0()
                                .child(icon(glyph, if chosen { p.ink } else { p.muted }))
                                .children(dot().map(|dot| dot.top(px(-2.)).right(px(-3.)))),
                        )
                        .when(self.nav_expanded, |tab| {
                            tab.child(div().min_w_0().truncate().child(name.clone()))
                        })
                })
        };
        if top {
            let tabs = gpui_kit::base::Tabs::new("page-navigation")
                .relative()
                .flex()
                .gap(px(2.))
                .on_prepaint(track_region(
                    0,
                    self.navigation_regions.clone(),
                    owner.clone(),
                ))
                .children(
                    PAGES
                        .iter()
                        .enumerate()
                        .map(|(index, (page, glyph))| entry(index, page, glyph)),
                )
                .when(regions[selected + 1].size.width > px(0.), |tabs| {
                    tabs.child(
                        div()
                            .absolute()
                            .bottom(px(2.))
                            .left(marker_x)
                            .w(marker_width)
                            .h(theme::dimension("navigationIndicatorHeight"))
                            .rounded(px(2.))
                            .bg(p.accent),
                    )
                });
            return div()
                .px(px(12.))
                .pb(px(6.))
                .flex_shrink_0()
                .child(tabs)
                .into_any_element();
        }
        let toggle_tip = label(if self.nav_expanded {
            "shell.nav.collapse"
        } else {
            "shell.nav.expand"
        });
        let mut items = gpui_kit::base::Tabs::new("page-navigation")
            .relative()
            .flex()
            .flex_col()
            .flex_1()
            .gap(px(4.))
            .on_prepaint(track_region(
                0,
                self.navigation_regions.clone(),
                owner.clone(),
            ));
        for (index, (page, glyph)) in PAGES.iter().enumerate() {
            if *page == "settings" {
                items = items.child(div().flex_1());
            }
            items = items.child(entry(index, page, glyph));
        }
        if regions[selected + 1].size.height > px(0.) {
            items = items.child(
                div()
                    .absolute()
                    .left_0()
                    .top(marker_y)
                    .w(px(3.))
                    .h(px(16.))
                    .rounded(px(2.))
                    .bg(p.accent),
            );
        }
        div()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .gap(px(4.))
            .px(px(4.))
            .pb(px(8.))
            .w(px(if self.nav_expanded { 184. } else { 48. }))
            .child(
                Button::new("nav-toggle")
                    .h(px(36.))
                    .px(px(12.))
                    .flex()
                    .items_center()
                    .rounded(px(4.))
                    .hover(move |s| s.bg(hover))
                    .active(move |s| s.bg(press))
                    .accessibility_label(toggle_tip.clone())
                    .child(icon("MenuIcon", p.muted))
                    .tooltip(move |window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new(toggle_tip.clone())
                            .build(window, cx)
                    })
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.act(
                            Action::Preference("navExpanded".into(), json!(!this.nav_expanded)),
                            window,
                            cx,
                        )
                    })),
            )
            .child(items)
            .into_any_element()
    }
    /// A toolbar button in the original `.btn` style: 32 px, a stroke border with a
    /// stronger bottom edge, the control surface, and the accent while pressed.
    /// Without text it is a square icon button.
    pub(in crate::frontend) fn tool_button(
        &self,
        id: impl Into<SharedString>,
        text: impl Into<SharedString>,
        glyph: Option<&str>,
        action: Action,
        pressed: bool,
        cx: &Context<Self>,
    ) -> Button {
        let text = text.into();
        let p = self.palette;
        let color = if pressed { p.accent } else { p.ink };
        let (rest, hover, press) = if pressed {
            (
                p.accent.opacity(0.16),
                p.accent.opacity(0.22),
                p.accent.opacity(0.12),
            )
        } else {
            (p.soft, p.soft_hover, p.soft_press)
        };
        Button::new(id.into())
            .relative()
            .h(px(32.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .gap(px(6.))
            .when(text.is_empty(), |b| b.w(px(32.)))
            .when(!text.is_empty(), |b| b.px(px(13.)))
            .rounded(px(6.))
            .bg(rest)
            .text_color(color)
            .hover(move |s| s.bg(hover))
            .active(move |s| s.bg(press))
            .accessibility_label(text.clone())
            .when_some(glyph, |b, name| b.child(icon(name, color)))
            .when(!text.is_empty(), |b| b.child(text))
            .on_click(cx.listener(move |this, _, window, cx| this.act(action.clone(), window, cx)))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
    }
    pub(in crate::frontend) fn toolbar(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.palette;
        let wide = window.viewport_size().width >= px(1100.);
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
            .gap_2()
            .flex_shrink_0()
            .occlude()
            .child(
                self.tool_button(
                    "task",
                    if wide {
                        label("tasks.title")
                    } else {
                        String::new()
                    },
                    Some("RunTaskIcon"),
                    Action::Dialog("task".into(), json!({})),
                    false,
                    cx,
                )
                .accessibility_label(label("tasks.title"))
                .tooltip(|window, cx| {
                    gpui_kit::component::tooltip::Tooltip::new(label("tasks.title"))
                        .build(window, cx)
                }),
            );
        if self.page == "services" {
            trailing = trailing.child(self.service_actions(wide, cx));
        }
        if sampled {
            const INTERVALS: [u64; 4] = [500, 1000, 2000, 5000];
            trailing = trailing.child(
                self.select(
                    "interval",
                    label("shell.interval.label"),
                    INTERVALS
                        .iter()
                        .map(|ms| tr!("shell.interval.option", seconds = *ms as f64 / 1000.))
                        .collect(),
                    INTERVALS.iter().position(|ms| *ms == self.interval),
                    false,
                    |this, index, window, cx| {
                        this.act(Action::Interval(INTERVALS[index]), window, cx);
                    },
                    cx,
                ),
            );
            let pause_tip = if self.paused {
                label("shell.resume")
            } else {
                label("shell.pause")
            };
            trailing = trailing.child(
                self.tool_button(
                    "pause",
                    String::new(),
                    Some(if self.paused { "PlayIcon" } else { "PauseIcon" }),
                    Action::Pause,
                    self.paused,
                    cx,
                )
                .accessibility_label(pause_tip.clone())
                .tooltip(move |window, cx| {
                    gpui_kit::component::tooltip::Tooltip::new(pause_tip.clone()).build(window, cx)
                }),
            );
        }
        trailing = trailing.child(
            self.tool_button(
                "lock",
                String::new(),
                Some("LockIcon"),
                Action::Dialog("filelock".into(), json!({})),
                false,
                cx,
            )
            .accessibility_label(label("shell.fileLock.label"))
            .tooltip(|window, cx| {
                gpui_kit::component::tooltip::Tooltip::new(label("shell.fileLock.title"))
                    .build(window, cx)
            }),
        );
        if self.page == "processes" {
            let tip = label(if self.details_open {
                "shell.details.hide"
            } else {
                "shell.details.show"
            });
            trailing = trailing.child(
                self.tool_button(
                    "details",
                    String::new(),
                    Some("PanelIcon"),
                    Action::Details,
                    self.details_open,
                    cx,
                )
                .accessibility_label(label("shell.details.label"))
                .tooltip(move |window, cx| {
                    gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
                }),
            );
        }
        if let Some(updates) = self
            .data
            .get("updates")
            .filter(|s| matches!(text(s, "state"), "ready" | "available" | "downloading"))
            .filter(|s| {
                self.prefs.get::<String>("updateDismissed", String::new()) != text(s, "version")
            })
        {
            // An icon filled with the accent; the version is in the tooltip.
            let title = if text(updates, "state") == "available" {
                tr!(
                    "updates.button.availableTitle",
                    version = text(updates, "version")
                )
            } else {
                tr!("updates.button.title", version = text(updates, "version"))
            };
            let on_accent = p.on_accent;
            trailing = trailing.child(
                Button::new("update-status")
                    .size(px(32.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(p.accent)
                    .hover(|s| s.opacity(0.92))
                    .accessibility_label(title.clone())
                    .child(icon("UpdateIcon", on_accent))
                    .tooltip(move |window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new(title.clone()).build(window, cx)
                    })
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.act(Action::Dialog("update".into(), json!({})), window, cx)
                    }))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_mouse_down(MouseButton::Right, {
                        let version = text(updates, "version").to_owned();
                        cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            let items = vec![
                                MenuItem::new(
                                    label("updates.menu.view"),
                                    Action::Dialog("update".into(), json!({})),
                                )
                                .icon("UpdateIcon"),
                                MenuItem::new(
                                    label("updates.menu.dismiss"),
                                    Action::Preference("updateDismissed".into(), json!(version)),
                                )
                                .icon("DismissIcon"),
                            ];
                            this.act(Action::Popup(items), window, cx);
                        })
                    }),
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
                    .gap(px(10.))
                    .pl(px(4.))
                    .when(!self.nav_top, |d| {
                        d.min_w(px(if self.nav_expanded { 184. } else { 48. }) - px(8.))
                    })
                    .text_size(px(12.))
                    .text_color(p.ink)
                    .child(img("argus/logo.svg").size(px(18.)))
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
                            .pt(px(14.))
                            .pb(px(10.))
                            .pl(px(12.))
                            .pr(px(4.))
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
                let filled = !self.query.trim().is_empty();
                let focused = self.search.read(cx).focus_handle(cx).is_focused(window);
                // A soft field: a quiet fill, an accent outline while focused, and an
                // accent tint while it filters the list.
                let hover = p.soft_hover;
                let (border, background) = if filled {
                    (
                        p.accent.opacity(if focused { 0.7 } else { 0.4 }),
                        p.soft.blend(p.accent.opacity(0.12)),
                    )
                } else if focused {
                    (p.accent.opacity(0.7), p.soft_hover)
                } else {
                    (p.soft.opacity(0.), p.soft)
                };
                toolbar = toolbar.child(
                    div()
                        .id("toolbar-search")
                        .absolute()
                        .left(x)
                        .top(px(14.))
                        .h(px(32.))
                        .w(width)
                        .flex()
                        .items_center()
                        .rounded(px(8.))
                        .border_1()
                        .border_color(border)
                        .bg(background)
                        .when(!filled && !focused, |d| d.hover(move |s| s.bg(hover)))
                        .overflow_hidden()
                        .occlude()
                        .child(
                            Input::new(&self.search)
                                .appearance(false)
                                .cleanable(true)
                                .h_full()
                                .pl(px(10.))
                                .prefix(icon(
                                    "SearchIcon",
                                    if filled { p.accent } else { p.muted },
                                )),
                        ),
                );
            }
        }
        toolbar.into_any_element()
    }
    /// What can be done to the selected service, after "Run new task".
    fn service_actions(&self, wide: bool, cx: &Context<Self>) -> Div {
        let services = self.data.get("services").map(values).unwrap_or_default();
        let selected = self
            .selected
            .get("services")
            .and_then(|name| services.iter().find(|s| text(s, "name") == name));
        let pending = selected.is_none_or(|s| text(s, "state").ends_with("Pending"));
        let mut row = div().flex().items_center().gap_2();
        for (id, glyph, action, title, disabled) in [
            (
                "svc-start",
                "PlayIcon",
                "start",
                "common.start",
                pending || selected.is_some_and(|s| text(s, "state") == "running"),
            ),
            (
                "svc-stop",
                "StopIcon",
                "stop",
                "common.stop",
                pending || !selected.is_some_and(|s| boolean(s, "canStop")),
            ),
            (
                "svc-restart",
                "RestartIcon",
                "restart",
                "common.restart",
                pending || !selected.is_some_and(|s| boolean(s, "canStop")),
            ),
        ] {
            let name = selected
                .map(|s| text(s, "name").to_owned())
                .unwrap_or_default();
            let tip = label(title);
            row = row.child(
                self.tool_button(
                    id,
                    String::new(),
                    Some(glyph),
                    Action::Invoke(
                        "control_service".into(),
                        json!({"name": name, "action": action}),
                    ),
                    false,
                    cx,
                )
                .disabled(disabled)
                .when(disabled, |b| b.opacity(0.4))
                .accessibility_label(label(&format!("services.control.{action}")))
                .tooltip(move |window, cx| {
                    gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
                }),
            );
        }
        let p = self.palette;
        let hover = p.hover;
        row.child(
            Button::new("services-console")
                .h(px(32.))
                .px(px(12.))
                .flex()
                .items_center()
                .gap(px(6.))
                .rounded(px(4.))
                .text_color(p.ink)
                .hover(move |s| s.bg(hover))
                .accessibility_label(label("services.control.openConsoleTitle"))
                .child(icon("ExternalIcon", p.ink))
                .when(wide, |b| b.child(label("services.control.openConsole")))
                .tooltip(|window, cx| {
                    gpui_kit::component::tooltip::Tooltip::new(label(
                        "services.control.openConsoleTitle",
                    ))
                    .build(window, cx)
                })
                .on_click(cx.listener(|this, _, window, cx| {
                    this.act(
                        Action::Invoke("open_services_console".into(), json!({})),
                        window,
                        cx,
                    )
                })),
        )
        .child(div().w(px(1.)).h(px(20.)).mx_1().bg(p.stroke))
    }
    pub(in crate::frontend) fn statusbar(&self) -> AnyElement {
        let p = self.palette;
        let sys = &self.snapshot["system"];
        let mut figures = div()
            .flex()
            .flex_wrap()
            .flex_1()
            .min_w_0()
            .h(px(17.))
            .overflow_hidden()
            .gap_x(px(20.));
        let cached = format!("{:.1} GB", number(sys, "cached") / 1073741824.);
        for (key, value) in [
            (
                "shell.status.threads",
                theme::count(number(sys, "threadCount")),
            ),
            (
                "shell.status.handles",
                theme::count(number(sys, "handleCount")),
            ),
            ("shell.status.cached", cached),
            (
                "shell.status.pagedPool",
                theme::memory(number(sys, "kernelPaged")),
            ),
            (
                "shell.status.uptime",
                status_uptime(number(sys, "uptimeMs") / 1000.),
            ),
        ] {
            figures = figures.child(
                div()
                    .flex()
                    .gap_1()
                    .whitespace_nowrap()
                    .child(label(key))
                    .child(div().text_color(p.muted).child(value)),
            );
        }
        let privilege = if boolean(&self.data["info"], "elevated") {
            "admin"
        } else if self.data.get("privilege").is_some_and(|s| {
            text(s, "state") == "running" && boolean(s, "authorized") && s["error"].is_null()
        }) {
            "service"
        } else {
            "standard"
        };
        let badge_color = match privilege {
            "admin" => p.accent,
            "service" => p.network,
            _ => p.muted,
        };
        let badge_fill = if privilege == "standard" {
            p.soft
        } else {
            badge_color.opacity(0.14)
        };
        let hint = label(&format!("shell.elevation.{privilege}Hint"));
        div()
            .pt(px(6.))
            .pb(px(8.))
            .px(px(16.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(20.))
            .text_size(px(12.))
            .text_color(p.faint)
            .whitespace_nowrap()
            .child(figures)
            .when(
                self.held_order && self.pointer_in_table && !self.paused,
                |d| {
                    d.child(
                        div()
                            .id("order-held")
                            .child(label("shell.status.orderHeld"))
                            .tooltip(|window, cx| {
                                gpui_kit::component::tooltip::Tooltip::new(label(
                                    "shell.status.orderHeldHint",
                                ))
                                .build(window, cx)
                            }),
                    )
                },
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(div().size(px(6.)).rounded_full().bg(if self.paused {
                        p.faint
                    } else {
                        p.cpu
                    }))
                    .child(if self.paused {
                        tr!("shell.status.paused")
                    } else {
                        tr!(
                            "shell.status.interval",
                            seconds = self.interval as f64 / 1000.
                        )
                    }),
            )
            .child(
                div()
                    .id("privilege-badge")
                    .flex()
                    .items_center()
                    .gap(px(5.))
                    .h(px(20.))
                    .my(px(-2.))
                    .px(px(8.))
                    .bg(badge_fill)
                    .rounded_full()
                    .text_color(badge_color)
                    .child(icon("ShieldIcon", badge_color).size(px(12.)))
                    .child(label(&format!("shell.elevation.{privilege}")))
                    .tooltip(move |window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new(hint.clone()).build(window, cx)
                    }),
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
                self.details_width = Self::clamp_details_width(
                    *start - f32::from(event.position.x - origin.x),
                    window,
                );
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
        let footer = self.statusbar();
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

/// Up time in the status bar, in words: days and hours, hours and minutes, or minutes and seconds.
fn status_uptime(seconds: f64) -> String {
    let s = seconds as u64;
    let (days, hours, minutes) = (s / 86400, s % 86400 / 3600, s % 3600 / 60);
    if days > 0 {
        tr!("format.uptime.days", days = days, hours = hours)
    } else if hours > 0 {
        tr!("format.uptime.hours", hours = hours, minutes = minutes)
    } else {
        tr!("format.uptime.minutes", minutes = minutes, seconds = s % 60)
    }
}
