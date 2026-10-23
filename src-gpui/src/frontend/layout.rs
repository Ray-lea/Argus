//! Application layout uses the native component rhythm and theme roles.
use super::*;
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
            .px_3()
            .py_2()
            .gap_2()
            .rounded(cx.theme().radius)
            .text_color(if selected { p.ink } else { p.muted })
            .when(selected, |b| b.bg(p.control))
            .hover(move |s| s.bg(p.hover).text_color(p.ink))
            .accessibility_label(text.clone())
            .when_some(glyph, |b, name| b.child(icon(name, p.muted)))
            .when(!text.is_empty(), |b| b.child(text))
            .on_click(cx.listener(move |this, _, window, cx| this.act(action.clone(), window, cx)))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
    }
    pub(in crate::frontend) fn navigation(&self, top: bool, cx: &Context<Self>) -> AnyElement {
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
        let mut controls = div().flex().items_center().gap_2().flex_shrink_0();
        if self.page == "processes" {
            for (mode, glyph) in [
                ("apps", "AppsIcon"),
                ("tree", "TreeIcon"),
                ("list", "ListIcon"),
            ] {
                controls = controls.child(
                    self.button(
                        format!("view-{mode}"),
                        if window.viewport_size().width >= window.rem_size() * 80. {
                            label(&format!("shell.viewMode.{mode}.label"))
                        } else {
                            String::new()
                        },
                        Some(glyph),
                        Action::View(mode.into()),
                        self.view == mode,
                        cx,
                    )
                    .accessibility_label(label(&format!("shell.viewMode.{mode}.label")))
                    .tooltip(move |window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new(label(&format!(
                            "shell.viewMode.{mode}.label"
                        )))
                        .build(window, cx)
                    }),
                );
            }
        } else {
            controls = controls.child(self.page_controls(window, cx));
        }
        let mut trailing = div().flex().items_center().gap_1().flex_shrink_0();
        let intervals = [500, 1000, 2000, 5000]
            .into_iter()
            .map(|ms| {
                MenuItem::new(
                    tr!("shell.interval.option", seconds = ms as f64 / 1000.),
                    Action::Interval(ms),
                )
            })
            .collect();
        trailing = trailing.child(self.button(
            "interval",
            tr!(
                "shell.interval.option",
                seconds = self.interval as f64 / 1000.
            ),
            Some("ChevronDown"),
            Action::Popup(intervals),
            false,
            cx,
        ));
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
                "task",
                "tasks.title",
                "RunTaskIcon",
                Action::Dialog("task".into(), json!({})),
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
            if id == "details" && self.page != "processes" {
                continue;
            }
            trailing = trailing.child(
                self.button(id, "", Some(glyph), action, false, cx)
                    .accessibility_label(label(key))
                    .tooltip(move |window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new(label(key)).build(window, cx)
                    }),
            );
        }
        if self
            .data
            .get("updates")
            .is_some_and(|s| matches!(text(s, "state"), "ready" | "available" | "downloading"))
        {
            trailing = trailing.child(
                self.button(
                    "update-status",
                    "",
                    Some("DownloadIcon"),
                    Action::Dialog("update".into(), json!({})),
                    false,
                    cx,
                )
                .accessibility_label(label("settings.update.check")),
            );
        }
        let search = div().flex_1().min_w_0().max_w_96().when(
            matches!(
                self.page.as_str(),
                "processes" | "services" | "startup" | "network"
            ),
            |d| d.child(Input::new(&self.search).cleanable(true)),
        );
        TitleBar::new()
            .h_auto()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .py_2()
                    .flex_1()
                    .min_w_0()
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
                    .child(div().flex_1())
                    .child(search)
                    .child(div().flex_1())
                    .child(trailing),
            )
            .into_any_element()
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
            .py_2()
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
            .child(self.button(
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
            ))
            .into_any_element()
    }
    pub(in crate::frontend) fn mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some((kind, origin, start)) = &self.drag
            && kind == "details"
        {
            let em = f32::from(window.rem_size());
            self.details_width = (*start - f32::from(event.position.x - origin.x))
                .clamp(em * 12., f32::from(window.viewport_size().width) * 0.45);
            cx.notify();
        }
    }
}
impl Render for Desktop {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.active_charts.borrow_mut().clear();
        let p = self.palette;
        let toolbar = self.toolbar(window, cx);
        let content = self.page_content(window, cx);
        let footer = self.statusbar(cx);
        let body = div()
            .flex_1()
            .min_h_0()
            .flex()
            .when(!self.nav_top, |d| d.child(self.navigation(false, cx)))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .child(content)
                    .child(footer),
            );
        let overlays = self.render_overlays(window, cx);
        self.schedule_chart_tick(window, cx);
        div()
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
                    if let Some((kind, _, _)) = this.drag.take()
                        && kind == "details"
                    {
                        this.prefs.set("detailsWidth", this.details_width);
                    }
                    cx.notify();
                }),
            )
            .child(toolbar)
            .when(self.nav_top, |d| d.child(self.navigation(true, cx)))
            .child(body)
            .children(overlays)
    }
}
