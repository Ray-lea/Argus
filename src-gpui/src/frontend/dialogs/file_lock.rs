//! The file-lock dialog: paths dropped or picked, the processes holding them,
//! and ending them (the desktop's own Explorer is restarted instead).
use super::*;

/// A locker's action button.
#[derive(Clone, Copy, Debug, PartialEq)]
struct LockerAction {
    /// The desktop's own Explorer: restarted rather than ended, so the desktop
    /// and taskbar come back.
    restart: bool,
    /// Locale key of the label.
    text: &'static str,
    glyph: &'static str,
    danger: bool,
    disabled: bool,
}

/// The action for `locker`, given the desktop shell (`desktop_shell`) and
/// whether an Explorer restart is under way.
fn locker_action(locker: &Value, shell: Option<&Value>, restarting: bool) -> LockerAction {
    let restart = shell.is_some_and(|shell| {
        shell.is_object()
            && shell["pid"] == locker["pid"]
            && shell["createTime"] == locker["createTime"]
    });
    LockerAction {
        restart,
        text: match (restart, restarting) {
            (false, _) => "fileLock.end",
            (true, false) => "tasks.explorer.restart",
            (true, true) => "tasks.explorer.restarting",
        },
        glyph: if restart { "RestartIcon" } else { "EndIcon" },
        danger: !restart,
        disabled: number(locker, "createTime") <= 0. || (restart && restarting),
    }
}

impl Desktop {
    pub(in crate::frontend) fn accept_dropped_files(
        &mut self,
        paths: &ExternalPaths,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let paths = paths
            .paths()
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        cx.stop_propagation();
        self.accept_file_paths(paths, window, cx);
    }

    pub(in crate::frontend) fn accept_file_paths(
        &mut self,
        paths: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if paths.is_empty() {
            return;
        }
        match self.modal.as_ref().map(|modal| modal.kind.as_str()) {
            Some("filelock") => self.search_lockers(paths, window, cx),
            None => self.open_dialog("filelock", json!({"paths":paths}), window, cx),
            _ => {}
        }
    }

    /// Checks the chosen paths for lockers.
    pub(in crate::frontend) fn search_lockers(
        &mut self,
        paths: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if paths.is_empty() {
            return;
        }
        if let Some(modal) = &mut self.modal
            && modal.kind == "filelock"
        {
            modal.args["paths"] = json!(paths);
            modal.error = None;
        }
        self.ui.lock_searches += 1;
        self.data.remove("lockers");
        self.request(
            "find_lockers",
            json!({"paths":paths}),
            Some("lockers"),
            window,
            cx,
        );
        // The desktop's own Explorer is offered a restart instead of an end.
        self.request("desktop_shell", json!({}), Some("desktopShell"), window, cx);
        window.refresh();
    }

    /// Checks the file-lock dialog's paths again, while it is open.
    pub(super) fn recheck_lockers(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(paths) = self
            .modal
            .as_ref()
            .filter(|m| m.kind == "filelock")
            .map(|m| m.args["paths"].clone())
        {
            let paths = values(&paths)
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect();
            self.search_lockers(paths, window, cx);
        }
    }

    /// "Who is using this file?": the chosen paths or a
    /// drop zone, the pickers, then the search, its error or its result.
    pub(super) fn filelock_dialog(
        &mut self,
        dialog: Dialog,
        modal: &Modal,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Dialog {
        let p = self.palette;
        let id = modal.id;
        let paths = values(&modal.args["paths"])
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let searching = self
            .pending
            .values()
            .any(|pending| pending.cache.as_deref() == Some("lockers"));
        // `.lock-result`: kept 48 px high so the dialog does not jump while
        // the search starts and ends.
        let mut result = div().mt(px(16.)).min_h(px(48.)).flex().flex_col();
        if searching {
            result = result
                .child(div().text_color(p.muted).child(label("fileLock.checking")))
                .child(div().mt(px(10.)).child(self.progress_bar(None, cx)));
        } else if let Some(error) = modal.error.clone() {
            // `.setting-error`
            result = result.child(
                div()
                    .mt(px(4.))
                    .text_size(theme::font(12.))
                    .text_color(p.danger)
                    .child(error),
            );
        } else if let Some(report) = self.data.get("lockers").cloned() {
            result = result.child(self.lock_report(&report, cx));
        }
        let has_paths = !paths.is_empty();
        // `.lock-pick`
        let picker = div()
            .flex()
            .gap(px(8.))
            .child(
                self.btn(
                    format!("modal-{id}-file"),
                    label("fileLock.pickFile"),
                    None,
                    Tone::Plain,
                )
                .on_click(
                    cx.listener(|this, _, window, cx| this.pick_lock_path(false, window, cx)),
                ),
            )
            .child(
                self.btn(
                    format!("modal-{id}-folder"),
                    label("fileLock.pickFolder"),
                    Some("FolderIcon"),
                    Tone::Plain,
                )
                .on_click(cx.listener(|this, _, window, cx| this.pick_lock_path(true, window, cx))),
            )
            .when(has_paths, |d| {
                let again = paths.clone();
                d.child(
                    self.btn(
                        format!("modal-{id}-recheck"),
                        label("fileLock.recheck"),
                        None,
                        Tone::Ghost,
                    )
                    .inert(searching)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.search_lockers(again.clone(), window, cx)
                    })),
                )
            });
        // `.lock-paths`: a card per path (name over full path), or the drop zone.
        let chosen = div()
            .mt(px(16.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .when(!has_paths, |d| {
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .px(px(16.))
                        .py(px(14.))
                        .rounded(px(6.))
                        .border_1()
                        .border_dashed()
                        .border_color(p.stroke_strong)
                        .text_color(p.muted)
                        .child(icon("LockIcon", p.muted).size(px(20.)).flex_shrink_0())
                        .child(label("fileLock.dropZone")),
                )
            })
            .children(paths.iter().map(|path| {
                let name = path
                    .rsplit(['\\', '/'])
                    .find(|part| !part.is_empty())
                    .unwrap_or(path)
                    .to_owned();
                div()
                    .flex()
                    .flex_col()
                    .min_w_0()
                    .px(px(12.))
                    .py(px(8.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(p.stroke)
                    .bg(p.layer)
                    .child(
                        div()
                            .truncate()
                            .font_weight(theme::LABEL_WEIGHT)
                            .child(name),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_size(theme::font(12.))
                            .text_color(p.faint)
                            .child(path.clone()),
                    )
            }))
            .child(picker);
        dialog
            .child(
                self.dialog_body(id)
                    .id(("filelock-drop", id))
                    .on_drop(cx.listener(Self::accept_dropped_files))
                    .child(
                        div()
                            .pt(px(22.))
                            .pl(px(24.))
                            // The body keeps its scrollbar's gutter (`scrollbar-gutter: stable`).
                            .pr(px(24. + scrollbar::SIZE))
                            .pb(px(20.))
                            .flex()
                            .flex_col()
                            .line_height(relative(1.4))
                            .child(
                                self.dialog_title(modal.title.clone())
                                    .line_height(relative(1.4))
                                    .mb(px(10.)),
                            )
                            .child(div().text_color(p.muted).child(modal.body.clone()))
                            .child(chosen)
                            .child(result),
                    ),
            )
            .footer(
                self.measure(id, 1).child(
                    self.dialog_footer()
                        .line_height(relative(1.4))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(theme::font(12.))
                                .text_color(p.faint)
                                .child(label("fileLock.footerHint")),
                        )
                        .child(
                            self.btn(
                                format!("modal-{id}-close"),
                                label("common.close"),
                                None,
                                Tone::Plain,
                            )
                            .min_w(px(96.))
                            .track_focus(&self.primary_focus(id, cx))
                            .on_click(
                                cx.listener(|this, _, window, cx| this.close_overlay(window, cx)),
                            ),
                        ),
                ),
            )
    }

    /// A finished search (`.lock-report`): how many processes use the paths,
    /// a card for each, and a boxed note per remark on what was checked.
    pub(super) fn lock_report(&self, report: &Value, cx: &mut Context<Self>) -> AnyElement {
        let p = self.palette;
        let lockers = values(&report["lockers"]);
        let mut column = div()
            .relative()
            .flex()
            .flex_col()
            .child(div().text_color(p.muted).child(if lockers.is_empty() {
                label("fileLock.none")
            } else {
                tr!("fileLock.found", count = theme::count(lockers.len() as f64))
            }));
        if !lockers.is_empty() {
            let shell = self.data.get("desktopShell");
            let restarting = self.ui.busy.contains("explorer");
            let scroll = scrollbar::fresh_handle("lockers", self.ui.lock_searches);
            // `.locker-list`: scrolls past 260 px and keeps its gutter.
            column = column.child(
                div()
                    .id("lockers")
                    .track_scroll(&scroll)
                    .mt(px(8.))
                    .max_h(px(260.))
                    .overflow_y_scroll()
                    .pr(px(scrollbar::SIZE))
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .children(lockers.iter().enumerate().map(|(index, locker)| {
                        self.locker_row(index, locker, shell, restarting, cx)
                    }))
                    .child(scrollbar::scrollbar("lockers-scrollbar", &scroll, p.dark)),
            );
        }
        let note = |content: String| {
            div()
                .mt(px(14.))
                .px(px(10.))
                .py(px(8.))
                .rounded(px(6.))
                .bg(p.hover)
                .text_size(theme::font(12.))
                .text_color(p.muted)
                .child(content)
        };
        if let Some(handles) = report["handles"].as_object().map(|_| &report["handles"]) {
            let detail = if boolean(handles, "available") {
                argus_core::i18n::format(
                    if boolean(handles, "viaService") {
                        "fileLock.handles.service"
                    } else {
                        "fileLock.handles.local"
                    },
                    &[("count", &theme::count(number(handles, "checked")))],
                )
            } else {
                label("fileLock.handles.unfinished")
            };
            column = column.child(note(tr!(
                "fileLock.checked",
                count = theme::count(number(report, "filesChecked")),
                handles = detail
            )));
            if number(handles, "deniedProcesses") > 0.
                || number(handles, "skipped") > 0.
                || number(handles, "timeouts") > 0.
                || boolean(handles, "truncated")
            {
                column = column.child(note(tr!(
                    "fileLock.incomplete",
                    denied = handles["deniedProcesses"],
                    skipped = handles["skipped"],
                    timeouts = handles["timeouts"],
                    limit = if boolean(handles, "truncated") {
                        label("fileLock.limitReached")
                    } else {
                        String::new()
                    }
                )));
            }
            if !boolean(handles, "viaService") && !boolean(handles, "elevated") {
                column = column.child(note(label("fileLock.serviceHint")));
            }
            if let Some(error) = handles["error"].as_str() {
                column = column.child(note(error.to_owned()));
            }
        }
        if let Some(error) = report["restartError"].as_str() {
            column = column.child(note(error.to_owned()));
        }
        if number(report, "unchecked") > 0. {
            column = column.child(note(tr!(
                "fileLock.unchecked",
                count = theme::count(number(report, "unchecked"))
            )));
        }
        if boolean(report, "truncated") {
            column = column.child(note(tr!(
                "fileLock.truncated",
                count = theme::count(number(report, "filesChecked") + number(report, "unchecked"))
            )));
        }
        // Each result rises into place (`fade-up`, 250 ms).
        if cx.reduce_motion() {
            column.into_any_element()
        } else {
            column
                .with_animation(
                    ("lock-report", self.ui.lock_searches),
                    Animation::new(Duration::from_millis(250)).with_easing(theme::ease_out),
                    |column, delta| column.opacity(delta).top(px((1. - delta) * 6.)),
                )
                .into_any_element()
        }
    }

    /// One process using the paths (`.locker`): its icon, name, executable and
    /// PID, the handle's path, its kind and handle count, then "show process"
    /// and End, or Restart for the desktop's own Explorer.
    pub(super) fn locker_row(
        &self,
        index: usize,
        locker: &Value,
        shell: Option<&Value>,
        restarting: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let p = self.palette;
        let process = values(&self.snapshot["processes"])
            .iter()
            .find(|q| q["pid"] == locker["pid"] && q["createTime"] == locker["createTime"])
            .cloned();
        let action = locker_action(locker, shell, restarting);
        let kind = text(locker, "kind");
        let handles = number(locker, "handleCount");
        let app = text(locker, "appName").to_owned();
        let description = process
            .as_ref()
            .map(|q| text(q, "description").to_owned())
            .filter(|d| !d.is_empty())
            .unwrap_or_else(|| app.clone());
        let exe = process
            .as_ref()
            .map(|q| text(q, "name").to_owned())
            .unwrap_or_else(|| app.clone());
        let service = text(locker, "service");
        // The interface's source has an ideographic space here, but its JSX
        // compiler turns it into a plain one, which is what it shows.
        let mut identity = format!("{exe} PID {}", locker["pid"]);
        if !service.is_empty() {
            identity.push(' ');
            identity.push_str(&tr!("fileLock.service", name = service));
        }
        // ProcIcon: its own icon, else the app glyph, or the kernel one when
        // there is no path (a process the snapshot does not know has none).
        let glyph = match process.as_ref().filter(|q| q["icon"].as_u64().is_some()) {
            Some(process) => self.process_icon(Some(process), 16., cx),
            None => icon(
                if process.as_ref().is_none_or(|q| q["path"].is_null()) {
                    "KernelIcon"
                } else {
                    "AppIcon"
                },
                p.glyph,
            )
            .size(px(16.))
            .flex_shrink_0()
            .into_any_element(),
        };
        // `.tag`
        let tag = |content: String| {
            div()
                .flex_shrink_0()
                .px(px(6.))
                .border_1()
                .border_color(p.stroke_strong)
                .rounded_full()
                .text_size(theme::font(11.))
                .line_height(px(16.))
                .text_color(p.muted)
                .whitespace_nowrap()
                .child(content)
        };
        let reveal = locker.clone();
        let ending = locker.clone();
        div()
            .flex()
            .items_center()
            .gap(px(10.))
            .px(px(10.))
            .py(px(8.))
            .border_1()
            .border_color(p.stroke)
            .rounded(px(6.))
            .child(glyph)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .child(div().child(description))
                    .child(
                        div()
                            .text_size(theme::font(12.))
                            .text_color(p.faint)
                            .child(identity),
                    )
                    .when_some(
                        locker["handlePath"]
                            .as_str()
                            .filter(|path| !path.is_empty()),
                        |d, path| {
                            d.child(
                                div()
                                    .text_size(theme::font(12.))
                                    .text_color(p.faint)
                                    .child(path.to_owned()),
                            )
                        },
                    ),
            )
            .when(
                matches!(kind, "service" | "explorer" | "console" | "critical"),
                |d| d.child(tag(label(&format!("fileLock.kind.{kind}")))),
            )
            .when(handles > 0., |d| {
                d.child(tag(tr!(
                    "fileLock.handleCount",
                    count = theme::count(handles)
                )))
            })
            .child(
                self.btn_small_glyph(
                    format!("locker-go-{index}"),
                    "",
                    "GoToIcon",
                    14.,
                    Tone::Ghost,
                )
                .accessibility_label(label("fileLock.showProcess"))
                .tooltip(|window, cx| widgets::tooltip(label("fileLock.showProcess"), window, cx))
                .inert(process.is_none())
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.preference("revealProcess", reveal.clone(), window, cx);
                })),
            )
            .child(
                self.btn_small_glyph(
                    format!("locker-end-{index}"),
                    label(action.text),
                    action.glyph,
                    14.,
                    if action.danger {
                        Tone::Danger
                    } else {
                        Tone::Plain
                    },
                )
                .inert(action.disabled)
                .on_click(cx.listener(move |this, _, window, cx| {
                    if action.restart {
                        // Confirmed, the restart checks the paths again.
                        let mut shell = ending.clone();
                        shell["locker"] = json!(true);
                        this.open_dialog("restartExplorer", shell, window, cx);
                    } else {
                        this.end_locker(ending.clone(), window, cx);
                    }
                })),
            )
    }

    pub(super) fn pick_lock_path(
        &mut self,
        folder: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.options.fixture.is_some() {
            return;
        }
        self.request("pick_path", json!({"folder":folder}), None, window, cx);
    }

    /// Ends a locker after asking, then checks again.
    pub(super) fn end_locker(
        &mut self,
        locker: Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let process = values(&self.snapshot["processes"])
            .iter()
            .find(|q| q["pid"] == locker["pid"] && q["createTime"] == locker["createTime"])
            .cloned();
        let name = process
            .as_ref()
            .map(|q| text(q, "name").to_owned())
            .unwrap_or_else(|| text(&locker, "appName").to_owned());
        let pid = locker["pid"].clone();
        let create = locker["createTime"].clone();
        let label_name = name.clone();
        self.ask(
            tr!("actions.end.title", name = name),
            tr!("actions.end.body", pid = pid),
            label("common.endProcess"),
            true,
            move |this, window, cx| {
                this.ui
                    .errors
                    .insert("lockerEnd".into(), label_name.clone());
                // A fixture never ends anything; a test control (`resolve`)
                // answers in its place.
                if this.options.fixture.is_some() {
                    return;
                }
                this.request(
                    "kill_process",
                    json!({"pid":pid,"createTime":create,"force":false,"locker":true}),
                    None,
                    window,
                    cx,
                );
            },
            window,
            cx,
        );
    }
}

#[cfg(test)]
mod tests {
    // Named imports keep the glob-imported GPUI `test` macro out of scope.
    use super::{Value, json, locker_action};

    #[test]
    fn the_desktop_shell_is_restarted_instead_of_ended() {
        let shell = json!({"pid":8844,"createTime":1792249514645_i64});
        let explorer =
            json!({"pid":8844,"createTime":1792249514645_i64,"appName":"Windows 资源管理器"});
        let action = locker_action(&explorer, Some(&shell), false);
        assert!(action.restart);
        assert_eq!(action.text, "tasks.explorer.restart");
        assert_eq!(action.glyph, "RestartIcon");
        assert!(!action.danger && !action.disabled);
        // While a restart runs the button says so and waits.
        let restarting = locker_action(&explorer, Some(&shell), true);
        assert_eq!(restarting.text, "tasks.explorer.restarting");
        assert!(restarting.disabled);
    }

    #[test]
    fn other_lockers_and_other_explorers_are_ended() {
        let shell = json!({"pid":8844,"createTime":1792249514645_i64});
        // Another Explorer process (a separate window process), or the same PID reused.
        for locker in [
            json!({"pid":9100,"createTime":1792249514645_i64}),
            json!({"pid":8844,"createTime":1792249600000_i64}),
        ] {
            let action = locker_action(&locker, Some(&shell), true);
            assert!(!action.restart);
            assert_eq!(action.text, "fileLock.end");
            assert_eq!(action.glyph, "EndIcon");
            assert!(action.danger && !action.disabled);
        }
        // Without a known shell (none found, or not asked yet) nothing is spared
        // here; the host still refuses to end the desktop's Explorer.
        let explorer = json!({"pid":8844,"createTime":1792249514645_i64});
        assert!(!locker_action(&explorer, None, false).restart);
        assert!(!locker_action(&explorer, Some(&Value::Null), false).restart);
        // A process whose start time is unknown cannot be ended safely.
        assert!(locker_action(&json!({"pid":4,"createTime":0}), Some(&shell), false).disabled);
    }
}
