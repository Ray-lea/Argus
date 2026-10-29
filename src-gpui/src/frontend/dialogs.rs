use super::*;
use gpui_kit::base::ElementExt;
use gpui_kit::component::WindowExt;
use gpui_kit::component::dialog::{Dialog, DialogEntrance};
use widgets::{ButtonExt, Handler, Toast, ToastKind, Tone};

/// What confirming a generic confirmation does.
#[derive(Clone)]
pub(super) struct Ask {
    pub label: String,
    pub danger: bool,
    pub then: Handler,
}

#[derive(Clone)]
pub(super) struct Modal {
    pub id: u64,
    pub kind: String,
    pub args: Value,
    pub title: String,
    pub body: String,
    pub error: Option<String>,
    pub ask: Option<Ask>,
    /// The dialog's own focus target, focused when it opens.
    pub shell: Option<FocusHandle>,
}

/// Dialogs that open over whatever is open (ConfirmDialog in Overlays.tsx).
const STACKING: [&str; 6] = [
    "confirm",
    "end",
    "endTree",
    "restartExplorer",
    "closeConnection",
    "priority",
];
const PRIORITIES: [&str; 6] = [
    "idle",
    "belowNormal",
    "normal",
    "aboveNormal",
    "high",
    "realtime",
];
/// Executables that host other programs; a rule on one reaches them all.
const SHARED_HOSTS: [&str; 21] = [
    "msedgewebview2.exe",
    "svchost.exe",
    "dllhost.exe",
    "rundll32.exe",
    "conhost.exe",
    "runtimebroker.exe",
    "applicationframehost.exe",
    "taskhostw.exe",
    "taskhost.exe",
    "wudfhost.exe",
    "node.exe",
    "python.exe",
    "pythonw.exe",
    "java.exe",
    "javaw.exe",
    "dotnet.exe",
    "wscript.exe",
    "cscript.exe",
    "cmd.exe",
    "powershell.exe",
    "pwsh.exe",
];

pub(super) fn valid_task(command: &str) -> bool {
    !command.trim().is_empty() && command.len() <= 8192 && !command.contains(['\0', '\r', '\n'])
}

fn hex_code(code: &Value) -> Option<String> {
    code.as_i64()
        .filter(|code| *code != 0)
        .map(|code| tr!("tasks.code", code = format!("0x{:08X}", code as u32)))
}

/// A locker's action button (FileLockDialog.tsx).
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
    pub(in crate::frontend) fn open_selected_action(
        &mut self,
        kind: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(row) = self
            .selected
            .get(&self.page)
            .and_then(|key| self.rows.iter().find(|r| &r.key == key))
            .filter(|r| r.section.is_none())
        {
            self.open_dialog(
                kind,
                json!({"members":row.members,"name":row.title}),
                window,
                cx,
            );
        }
    }
    pub(in crate::frontend) fn form(
        &mut self,
        key: &str,
        value: &str,
        placeholder: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let value = value.to_owned();
        let placeholder = label(placeholder);
        self.forms.insert(
            key.into(),
            cx.new(|cx| {
                let mut state = InputState::new(window, cx).placeholder(placeholder);
                state.set_value(value, window, cx);
                state
            }),
        );
    }

    /// Asks before `then` (ConfirmDialog): over any dialog already open.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::frontend) fn ask(
        &mut self,
        title: String,
        body: String,
        confirm: String,
        danger: bool,
        then: impl Fn(&mut Desktop, &mut Window, &mut Context<Desktop>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let args = json!({});
        self.present(
            Modal {
                id: 0,
                kind: "confirm".into(),
                args,
                title,
                body,
                error: None,
                ask: Some(Ask {
                    label: confirm,
                    danger,
                    then: Rc::new(then),
                }),
                shell: None,
            },
            window,
            cx,
        );
    }

    pub(in crate::frontend) fn open_dialog(
        &mut self,
        kind: &str,
        args: Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_menu(window, cx);
        self.close_select();
        let name = text(&args, "name").to_owned();
        let members = values(&args["members"]).to_vec();
        let first = members.first().cloned().unwrap_or(Value::Null);
        let pid = first["pid"].to_string();
        // Process actions that apply at once, or ask first (actions.ts).
        match kind {
            "priority" if text(&args, "priority") != "realtime" => {
                self.process_batch("priority", &args, window, cx);
                return;
            }
            "efficiency" => {
                self.process_batch("efficiency", &args, window, cx);
                return;
            }
            "ruleScope" => {
                self.add_rule_for(&name, window, cx);
                return;
            }
            "hotkey" => {
                self.preference("settingsSection", json!("hotkey"), window, cx);
                self.begin_hotkey(window, cx);
                return;
            }
            "rule" => {
                self.preference("settingsSection", json!("rules"), window, cx);
                return;
            }
            "proxy" => {
                self.ui.proxy_draft = Some(text(&args, "updateProxy").to_owned());
                self.preference("settingsSection", json!("update"), window, cx);
                return;
            }
            "sensors" => {
                self.set_sensors(boolean(&args, "enabled"), window, cx);
                return;
            }
            "privilege" => {
                self.manage_service(text(&args, "action").to_owned(), window, cx);
                return;
            }
            "taskmgr" => {
                self.manage_task_manager(text(&args, "action").to_owned(), window, cx);
                return;
            }
            "clearHistory" => {
                self.clear_history(window, cx);
                return;
            }
            _ => {}
        }
        let count = members.len();
        let (title, body, ask) = match kind {
            "end" => {
                let group = count > 1;
                (
                    tr!("actions.end.title", name = name),
                    if group {
                        tr!("actions.endGroup.body", name = name, count = count)
                    } else {
                        tr!("actions.end.body", pid = pid)
                    },
                    Some((
                        label(if group {
                            "actions.endAll"
                        } else {
                            "common.endProcess"
                        }),
                        true,
                    )),
                )
            }
            "endTree" => {
                let descendants = self.descendants(&first);
                (
                    tr!("actions.endTree.title", name = name),
                    if descendants > 0 {
                        tr!("actions.endTree.body", pid = pid, count = descendants)
                    } else {
                        tr!("actions.endTree.bodyAlone", pid = pid)
                    },
                    Some((label("common.endProcessTree"), true)),
                )
            }
            "priority" => (
                tr!("actions.priority.confirmTitle", name = name),
                label("actions.priority.confirmBody"),
                Some((label("actions.priority.confirm"), true)),
            ),
            "restartExplorer" => (
                label("tasks.explorer.title"),
                label("tasks.explorer.confirm"),
                Some((label("tasks.explorer.restart"), false)),
            ),
            "closeConnection" => (
                label("connections.menu.close"),
                format!(
                    "{}:{} → {}:{}",
                    text(&args, "localAddress"),
                    args["localPort"],
                    text(&args, "remoteAddress"),
                    args["remotePort"]
                ),
                Some((label("connections.menu.close"), true)),
            ),
            "task" => (label("tasks.title"), label("tasks.description"), None),
            "filelock" => (label("fileLock.title"), label("fileLock.intro"), None),
            "note" => (label("common.marks.note"), name.clone(), None),
            "update" => (String::new(), String::new(), None),
            "license" => (
                "GNU Affero General Public License".into(),
                include_str!("../../../LICENSE").into(),
                None,
            ),
            "notices" => (
                label("settings.about.notices.dialog"),
                include_str!("../../../THIRD-PARTY-NOTICES.txt").into(),
                None,
            ),
            "shortcuts" => (label("settings.shortcuts.title"), String::new(), None),
            "onboarding" => (
                label("settings.about.onboarding.title"),
                String::new(),
                None,
            ),
            "whatsnew" => (
                tr!(
                    "updates.dialog.installed",
                    version = env!("CARGO_PKG_VERSION")
                ),
                String::new(),
                None,
            ),
            _ => return,
        };
        let stacking = STACKING.contains(&kind);
        if !stacking {
            let onboarding = self
                .modal
                .iter()
                .chain(&self.ui.below)
                .any(|m| m.kind == "onboarding");
            if onboarding && kind != "onboarding" {
                // Never on top of the onboarding, nor in its place: after it.
                if !self.ui.queued.iter().any(|(queued, _)| queued == kind) {
                    self.ui.queued.push((kind.to_owned(), args));
                }
                return;
            }
            self.close_all_modals(window, cx);
            self.forms.clear();
            match kind {
                "task" => {
                    self.form(
                        "value",
                        text(&args, "command"),
                        "tasks.placeholder",
                        window,
                        cx,
                    );
                    if let Some(input) = self.forms.get("value").cloned() {
                        self.ui.input_subscriptions.push(cx.subscribe_in(
                            &input,
                            window,
                            |this, _, event, window, cx| match event {
                                InputEvent::Change => {
                                    if let Some(modal) = &mut this.modal
                                        && modal.kind == "task"
                                    {
                                        modal.error = None;
                                        modal.args["failure"] = Value::Null;
                                    }
                                    window.refresh();
                                }
                                InputEvent::PressEnter { .. } => this.run_task(window, cx),
                                _ => {}
                            },
                        ));
                    }
                }
                "note" => self.form(
                    "value",
                    text(&self.marks()[text(&args, "key")], "note"),
                    "common.marks.placeholder",
                    window,
                    cx,
                ),
                "filelock" => {
                    // Stale results never survive a reopening.
                    self.data.remove("lockers");
                }
                _ => {}
            }
        }
        let paths = values(&args["paths"])
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let id = self.present(
            Modal {
                id: 0,
                kind: kind.into(),
                args,
                title,
                body,
                error: None,
                ask: ask.map(|(label, danger)| Ask {
                    label,
                    danger,
                    then: Rc::new(
                        |this: &mut Desktop, window: &mut Window, cx: &mut Context<Desktop>| {
                            this.confirm_dialog(window, cx)
                        },
                    ),
                }),
                shell: None,
            },
            window,
            cx,
        );
        if kind == "onboarding" {
            self.request(
                "privileged_service_status",
                json!({}),
                Some("privilege"),
                window,
                cx,
            );
            self.request(
                "task_manager_status",
                json!({}),
                Some("taskmgr"),
                window,
                cx,
            );
            self.request(
                "administrator_status",
                json!({}),
                Some("administrator"),
                window,
                cx,
            );
        }
        if kind == "filelock" && !paths.is_empty() {
            self.search_lockers(paths, window, cx);
        }
        let _ = id;
        if let Some(input) = self.forms.get("value").filter(|_| !stacking) {
            input.update(cx, |state, cx| state.focus(window, cx));
        }
        cx.notify();
    }

    /// Puts a dialog on screen, over the ones open; returns its id.
    fn present(&mut self, mut modal: Modal, window: &mut Window, cx: &mut Context<Self>) -> u64 {
        // A dialog still fading out goes at once rather than end up under this one.
        if let Some((id, _, _)) = self.ui.dialog_leaving {
            self.finish_leaving(id, window, cx);
        }
        self.ui.modal_seq += 1;
        let id = self.ui.modal_seq;
        modal.id = id;
        // The primary control takes the focus, as the original's buttons did.
        let primary = self.focus_for(&format!("modal-{id}-primary"), cx);
        if let Some(top) = self.modal.take() {
            self.ui.below.push(top);
        }
        let owner = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, window, cx| {
            if let Some(owner) = owner.upgrade() {
                owner.update(cx, |this, cx| this.dialog_view(id, dialog, window, cx))
            } else {
                dialog
            }
        });
        modal.shell = window.focused(cx);
        if modal.kind != "task" && modal.kind != "note" {
            primary.focus(window, cx);
        }
        self.modal = Some(modal);
        window.refresh();
        id
    }

    /// Escape, a click outside, or a close button: the top layer goes.
    pub(in crate::frontend) fn close_overlay(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.popup.is_some() || self.ui.select.is_some() {
            self.close_menu(window, cx);
            self.close_select();
            window.refresh();
            return;
        }
        self.begin_leaving(false, window, cx);
    }

    /// The top dialog fades out (`dialog-out`, with the scrim when it is the
    /// last one) and closes when that ends; at once under reduced motion.
    fn begin_leaving(&mut self, cancelled: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.modal.as_ref().map(|m| m.id) else {
            if self.modal.is_none() {
                self.focus.focus(window, cx);
            }
            return;
        };
        if self.ui.dialog_leaving.is_some() {
            return;
        }
        self.ui.dialog_leaving = Some((id, Instant::now(), cancelled));
        if cx.reduce_motion() {
            self.finish_leaving(id, window, cx);
            return;
        }
        if self.ui.below.is_empty() {
            self.scrim.leaving.set(Some(Instant::now()));
            self.notify_scrim(cx);
        }
        window.refresh();
        cx.notify();
    }

    /// Closes the dialog that was fading out.
    fn finish_leaving(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some((leaving, _, cancelled)) = self.ui.dialog_leaving else {
            return;
        };
        if leaving != id {
            return;
        }
        self.ui.dialog_leaving = None;
        self.ui.dialog_shown.borrow_mut().remove(&id);
        if let Some(closed) = self.modal.take() {
            window.close_dialog(cx);
            self.modal = self.ui.below.pop();
            self.after_close(closed, cancelled, window, cx);
        }
        if self.modal.is_none() {
            self.scrim.shown.set(None);
            self.scrim.leaving.set(None);
            self.notify_scrim(cx);
            self.focus.focus(window, cx);
        }
        window.refresh();
        cx.notify();
    }

    fn notify_scrim(&self, cx: &mut Context<Self>) {
        if let Some(scrim) = &self.scrim_view {
            scrim.update(cx, |_, cx| cx.notify());
        }
    }

    fn close_all_modals(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.modal.is_none() {
            return;
        }
        window.close_all_dialogs(cx);
        self.modal = None;
        self.ui.below.clear();
    }

    /// The dialog closed itself (Escape or a press outside it).
    fn modal_closed(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        if self.modal.as_ref().is_some_and(|m| m.id == id) {
            let closed = self.modal.take();
            self.modal = self.ui.below.pop();
            if let Some(closed) = closed {
                self.after_close(closed, true, window, cx);
            }
        } else {
            self.ui.below.retain(|m| m.id != id);
        }
        if self.modal.is_none() {
            self.focus.focus(window, cx);
        }
        cx.notify();
    }

    fn after_close(
        &mut self,
        closed: Modal,
        cancelled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if closed.kind == "onboarding" && cancelled {
            self.request(
                "complete_onboarding",
                json!({"version":welcome::configuration()["onboardingVersion"]}),
                Some("settings"),
                window,
                cx,
            );
        }
        if closed.kind == "task" {
            self.ui.input_subscriptions.clear();
        }
        if !self
            .modal
            .iter()
            .chain(&self.ui.below)
            .any(|m| m.kind == "onboarding")
            && !self.ui.queued.is_empty()
        {
            let queued = std::mem::take(&mut self.ui.queued);
            let owner = cx.entity().downgrade();
            window.defer(cx, move |window, cx| {
                let _ = owner.update(cx, |this, cx| {
                    for (kind, args) in queued {
                        this.open_dialog(&kind, args, window, cx);
                    }
                });
            });
        }
        window.refresh();
    }

    fn find_modal(&self, id: u64) -> Option<&Modal> {
        self.modal
            .iter()
            .chain(self.ui.below.iter())
            .find(|m| m.id == id)
    }

    /// Whether the dialog may close now (not while it runs something).
    fn may_close(&self, id: u64) -> bool {
        let Some(modal) = self.find_modal(id) else {
            return true;
        };
        match modal.kind.as_str() {
            "task" => !self.ui.busy.contains("task") && !self.ui.busy.contains("pick"),
            "update" => {
                !self.ui.busy.contains("install")
                    && text(self.data.get("updates").unwrap_or(&Value::Null), "state")
                        != "installing"
            }
            _ => true,
        }
    }

    /// Wraps a dialog's body (0) or footer (1) to measure it.
    pub(in crate::frontend) fn measure(&self, id: u64, slot: u8) -> Div {
        let parts = self.ui.dialog_parts.clone();
        div()
            .flex()
            .flex_col()
            .on_prepaint(move |bounds, window, cx| {
                let changed = parts.borrow().get(&(id, slot)) != Some(&bounds.size.height);
                if changed {
                    parts.borrow_mut().insert((id, slot), bounds.size.height);
                    window.defer(cx, |window, _| window.refresh());
                }
            })
    }

    /// The common dialog surface: width, colours, no close button, Fluent
    /// fade, centred in the window as the original scrim centred it.
    fn surface(
        &self,
        dialog: Dialog,
        id: u64,
        width: f32,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Dialog {
        let p = self.palette;
        let parts = self.ui.dialog_parts.borrow();
        let measured = parts.get(&(id, 0)).zip(parts.get(&(id, 1)));
        // Centred from the measured height. Until the first measurement the dialog
        // stays invisible, so it never shows at the default position and then jumps.
        // The component moves each stacked dialog 16 px lower; the original
        // centred a confirmation over the dialog below it as any other.
        let layer = self
            .ui
            .below
            .iter()
            .position(|m| m.id == id)
            .unwrap_or(self.ui.below.len());
        let dialog = match measured {
            Some((body, footer)) => {
                let height = *body + *footer + px(2.);
                let top = ((window.viewport_size().height - height) / 2.).max(px(16.));
                let dialog = dialog.margin_top(top - px(16. * layer as f32));
                dialog.opacity(self.dialog_opacity(id, window, cx))
            }
            None => dialog.opacity(0.),
        };
        let owner = cx.entity().downgrade();
        let enter = owner.clone();
        let cancel = owner.clone();
        dialog
            .w(px(width))
            .p_0()
            .gap_0()
            // The original's 13 px body text; the component's default is larger.
            .text_size(theme::font(13.))
            .bg(widgets::solid(p.flyout))
            .border_color(p.stroke_strong)
            .rounded(px(8.))
            .shadow(widgets::flyout_shadow(p.dark))
            .close_button(false)
            // The entrance, the exit and the scrim (`Scrim`, drawn over the chart
            // layer too) follow the original; the backdrop only takes the clicks.
            .entrance(DialogEntrance::None)
            .on_ok(move |_, window, cx| {
                let _ = enter.update(cx, |this, cx| this.dialog_enter(id, window, cx));
                false
            })
            .on_cancel(move |_, window, cx| {
                if let Some(this) = cancel.upgrade()
                    && this.read(cx).may_close(id)
                {
                    window.defer(cx, move |window, cx| {
                        this.update(cx, |this, cx| {
                            match this.modal.as_ref().filter(|m| m.id == id) {
                                // Escape counts as done, as the original's close().
                                Some(m) if m.kind == "onboarding" => {
                                    this.finish_onboarding(window, cx)
                                }
                                Some(_) => this.begin_leaving(true, window, cx),
                                None => {}
                            }
                        });
                    });
                }
                false
            })
            .on_close(move |_, window, cx| {
                let _ = owner.update(cx, |this, cx| this.modal_closed(id, window, cx));
            })
    }

    /// A shown dialog's opacity: in over 250 ms from its first visible frame
    /// (`dialog-in`, decelerating), out over 167 ms while it leaves
    /// (`dialog-out`, accelerating). The original's slight scale is left out.
    fn dialog_opacity(&self, id: u64, window: &Window, cx: &mut Context<Self>) -> f32 {
        let shown = *self
            .ui
            .dialog_shown
            .borrow_mut()
            .entry(id)
            .or_insert_with(Instant::now);
        if self.scrim.shown.get().is_none() {
            self.scrim.shown.set(Some(shown));
            self.scrim.leaving.set(None);
            self.notify_scrim(cx);
        }
        if cx.reduce_motion() {
            return 1.;
        }
        let entering = (shown.elapsed().as_secs_f32() / 0.25).min(1.);
        let mut opacity = theme::ease_out(entering);
        let mut animating = entering < 1.;
        if let Some((leaving, started, _)) = self.ui.dialog_leaving
            && leaving == id
        {
            let t = (started.elapsed().as_secs_f32() / 0.167).min(1.);
            opacity *= 1. - theme::ease_in(t);
            if t >= 1. {
                cx.defer_in(window, move |this, window, cx| {
                    this.finish_leaving(id, window, cx)
                });
            } else {
                animating = true;
            }
        }
        if animating {
            window.request_animation_frame();
        }
        opacity
    }

    /// A dialog's title in the display face (`.dialog-title`).
    pub(in crate::frontend) fn dialog_title(&self, title: impl Into<SharedString>) -> Div {
        div()
            .font_family(theme::DISPLAY_FONT)
            .text_size(theme::font(20.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(self.palette.ink)
            .child(title.into())
    }

    /// The footer strip (`.dialog-footer`): a top border on the layer colour.
    pub(in crate::frontend) fn dialog_footer(&self) -> Div {
        div()
            .flex()
            .items_center()
            .justify_end()
            .gap(px(8.))
            .px(px(24.))
            .py(px(16.))
            .border_t_1()
            .border_color(self.palette.stroke)
            .bg(self.palette.layer)
            .rounded_b(px(8.))
    }

    fn primary_focus(&self, id: u64, cx: &App) -> FocusHandle {
        self.focus_for(&format!("modal-{id}-primary"), cx)
    }

    /// A close button for read-only dialogs (accent, focused).
    fn close_button(&self, id: u64, cx: &mut Context<Self>) -> AnyElement {
        self.btn(
            format!("modal-{id}-close"),
            label("common.close"),
            None,
            Tone::Accent,
        )
        .min_w(px(96.))
        .track_focus(&self.primary_focus(id, cx))
        .on_click(cx.listener(|this, _, window, cx| this.close_overlay(window, cx)))
        .into_any_element()
    }

    pub(in crate::frontend) fn dialog_view(
        &mut self,
        id: u64,
        dialog: Dialog,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Dialog {
        let Some(modal) = self.find_modal(id).cloned() else {
            return dialog;
        };
        match modal.kind.as_str() {
            "onboarding" => {
                // Only its own buttons and Escape close the onboarding.
                let dialog = self
                    .surface(dialog, id, 580., window, cx)
                    .overlay_closable(false);
                self.onboarding_dialog(dialog, &modal, window, cx)
            }
            "shortcuts" => {
                let dialog = self.surface(dialog, id, 780., window, cx);
                self.shortcuts_dialog(dialog, id, window, cx)
            }
            "license" | "notices" => {
                let dialog = self.surface(dialog, id, 720., window, cx);
                self.document_dialog(dialog, &modal, cx)
            }
            "task" => {
                let dialog = self
                    .surface(dialog, id, 540., window, cx)
                    .bg(widgets::solid(theme::token("window-bg", self.palette.dark)));
                self.task_dialog(dialog, &modal, cx)
            }
            "filelock" => {
                let dialog = self.surface(dialog, id, 620., window, cx);
                self.filelock_dialog(dialog, &modal, window, cx)
            }
            "update" | "whatsnew" => {
                let dialog = self.surface(dialog, id, 620., window, cx);
                self.update_dialog(dialog, &modal, window, cx)
            }
            "note" => {
                let dialog = self.surface(dialog, id, 420., window, cx);
                self.note_dialog(dialog, &modal, cx)
            }
            _ => {
                let dialog = self.surface(dialog, id, 420., window, cx);
                self.confirm_view(dialog, &modal, cx)
            }
        }
    }

    /// A confirmation (Overlays.tsx, ConfirmDialog): title and text, then
    /// the accent or danger button first and Cancel.
    fn confirm_view(&self, dialog: Dialog, modal: &Modal, cx: &mut Context<Self>) -> Dialog {
        let p = self.palette;
        let id = modal.id;
        let ask = modal.ask.clone();
        let confirm = ask
            .as_ref()
            .map(|a| a.label.clone())
            .unwrap_or_else(|| label("common.close"));
        let danger = ask.as_ref().is_some_and(|a| a.danger);
        dialog
            .child(
                self.measure(id, 0).child(
                    div()
                        .pt(px(22.))
                        .px(px(24.))
                        .pb(px(20.))
                        .flex()
                        .flex_col()
                        .gap(px(10.))
                        .child(self.dialog_title(modal.title.clone()))
                        .child(
                            // `.dialog-text`, in the body's line height.
                            div()
                                .text_size(theme::font(13.))
                                .text_color(p.muted)
                                .child(modal.body.clone()),
                        )
                        .when_some(modal.error.clone(), |d, error| {
                            d.child(
                                div()
                                    .text_size(theme::font(12.))
                                    .text_color(p.danger)
                                    .child(error),
                            )
                        }),
                ),
            )
            .footer(
                self.measure(id, 1).child(
                    self.dialog_footer()
                        .child(
                            self.btn(
                                format!("modal-{id}-confirm"),
                                confirm,
                                None,
                                if danger { Tone::Danger } else { Tone::Accent },
                            )
                            .min_w(px(96.))
                            .track_focus(&self.primary_focus(id, cx))
                            .on_click(cx.listener(
                                move |this, _, window, cx| {
                                    this.answer(id, true, window, cx);
                                },
                            )),
                        )
                        .child(
                            self.btn(
                                format!("modal-{id}-cancel"),
                                label("common.cancel"),
                                None,
                                Tone::Plain,
                            )
                            .min_w(px(96.))
                            .on_click(cx.listener(
                                move |this, _, window, cx| {
                                    this.answer(id, false, window, cx);
                                },
                            )),
                        ),
                ),
            )
    }

    /// Answers the confirmation `id`: it closes, then confirming runs its step.
    fn answer(&mut self, id: u64, ok: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.modal.as_ref().is_none_or(|m| m.id != id) {
            return;
        }
        let modal = self.modal.clone();
        if ok
            && let Some(modal) = modal
            && let Some(ask) = modal.ask.clone()
        {
            if modal.kind == "confirm" {
                self.close_overlay(window, cx);
                (ask.then)(self, window, cx);
            } else {
                self.confirm_dialog(window, cx);
            }
        } else {
            self.close_overlay(window, cx);
        }
        cx.notify();
    }

    /// Enter in a dialog: the focused control acts on it; with the dialog
    /// itself or a text box focused, the dialog's main action runs.
    fn dialog_enter(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(modal) = self.find_modal(id).cloned() else {
            cx.propagate();
            return;
        };
        let focused = window.focused(cx);
        let on_shell = focused.is_none() || focused == modal.shell;
        let in_input = self
            .forms
            .values()
            .any(|form| form.read(cx).focus_handle(cx).is_focused(window));
        if !(on_shell || in_input) {
            // Let the focused button take its own click.
            cx.propagate();
            return;
        }
        match modal.kind.as_str() {
            "task" => self.run_task(window, cx),
            "note" => self.confirm_dialog(window, cx),
            "onboarding" => self.onboarding_next(window, cx),
            "license" | "notices" | "shortcuts" | "whatsnew" => self.close_overlay(window, cx),
            "filelock" | "update" => {}
            _ => self.act(Action::Confirm, window, cx),
        }
    }

    pub(in crate::frontend) fn confirm_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(modal) = self.modal.clone() else {
            return;
        };
        let value = self
            .forms
            .get("value")
            .map(|v| v.read(cx).value().to_string())
            .unwrap_or_default();
        let args = &modal.args;
        match modal.kind.as_str() {
            "confirm" => {
                if let Some(ask) = modal.ask {
                    self.close_overlay(window, cx);
                    (ask.then)(self, window, cx);
                }
                return;
            }
            "restartExplorer" => {
                self.close_overlay(window, cx);
                self.restart_explorer(args.clone(), window, cx);
                return;
            }
            "end" | "endTree" | "priority" => {
                self.close_overlay(window, cx);
                self.process_batch(&modal.kind, args, window, cx);
                return;
            }
            "task" => {
                self.run_task(window, cx);
                return;
            }
            "closeConnection" => self.request("close_connection", args.clone(), None, window, cx),
            "update" => {
                self.update_primary(window, cx);
                return;
            }
            "note" => self.set_mark(
                text(args, "key"),
                "note",
                json!(
                    value
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ")
                        .chars()
                        .take(80)
                        .collect::<String>()
                ),
                cx,
            ),
            "filelock" => return,
            _ => {}
        }
        self.close_overlay(window, cx);
    }

    /// The number of processes started under `root`, as the process tree counts them.
    fn descendants(&self, root: &Value) -> usize {
        let processes = values(&self.snapshot["processes"]);
        let mut children: HashMap<u64, Vec<&Value>> = HashMap::new();
        for p in processes {
            children
                .entry(p["ppid"].as_u64().unwrap_or(0))
                .or_default()
                .push(p);
        }
        let mut seen = HashSet::from([root["pid"].as_u64().unwrap_or(0)]);
        let mut stack = vec![root];
        let mut count = 0;
        while let Some(p) = stack.pop() {
            for c in children
                .get(&p["pid"].as_u64().unwrap_or(0))
                .into_iter()
                .flatten()
            {
                let pid = c["pid"].as_u64().unwrap_or(0);
                if number(c, "createTime") >= number(p, "createTime") && seen.insert(pid) {
                    count += 1;
                    stack.push(c);
                }
            }
        }
        count
    }

    /// Runs one process action on each member off the UI thread and reports
    /// the outcome once, as actions.ts does with Promise.allSettled.
    fn process_batch(
        &mut self,
        kind: &str,
        args: &Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let members = values(&args["members"]).to_vec();
        let name = text(args, "name").to_owned();
        if members.is_empty() {
            self.notify_toast(ToastKind::Error, label("actions.staleProcess"));
            return;
        }
        let (command, extra) = match kind {
            "end" => ("kill_process", json!({"force":false})),
            "endTree" => ("kill_process_tree", json!({})),
            "priority" => ("set_priority", json!({"priority":args["priority"]})),
            _ => ("set_efficiency_mode", json!({"on":args["on"]})),
        };
        let payloads = if kind == "endTree" {
            members.iter().take(1).cloned().collect::<Vec<_>>()
        } else {
            members
        }
        .into_iter()
        .map(|member| {
            let mut payload = json!({"pid":member["pid"],"createTime":member["createTime"]});
            for (key, value) in extra.as_object().into_iter().flatten() {
                payload[key] = value.clone();
            }
            payload
        })
        .collect::<Vec<_>>();
        if self.options.fixture.is_some() {
            return;
        }
        self.next_request += 1;
        let id = self.next_request;
        let summary = json!({"kind":kind,"command":command,"name":name,"priority":args["priority"],"on":args["on"]});
        self.pending.insert(
            id,
            Pending {
                command: "batch".into(),
                cache: None,
                args: summary.clone(),
            },
        );
        let host = self.host.clone();
        let command = command.to_owned();
        std::thread::spawn(move || {
            let results = payloads
                .into_iter()
                .map(|payload| {
                    let result = crate::dispatch::invoke(&host, &command, payload.clone());
                    json!({"payload":payload,"ok":result.is_ok(),"value":result.unwrap_or_else(|e| e)})
                })
                .collect::<Vec<_>>();
            let mut value = summary;
            value["results"] = json!(results);
            let _ = host.emit("command-response", json!({"id":id,"ok":true,"value":value}));
        });
        let _ = window;
        cx.notify();
    }

    fn batch_done(&mut self, value: &Value) {
        let kind = text(value, "kind");
        let command = text(value, "command");
        let name = text(value, "name").to_owned();
        let results = values(&value["results"]);
        let failed = results
            .iter()
            .filter(|r| !boolean(r, "ok"))
            .collect::<Vec<_>>();
        let total = results.len();
        let first_error = failed
            .first()
            .map(|r| self.action_error(command, &r["payload"], &r["value"]))
            .unwrap_or_default();
        let outdated = failed
            .iter()
            .any(|r| text(&r["value"], "kind") == "serviceOutdated");
        let (kind_out, text_out, ms) = match kind {
            "end" if total > 1 => {
                if failed.is_empty() {
                    (
                        ToastKind::Success,
                        tr!("actions.endGroup.done", name = name, count = total),
                        5000,
                    )
                } else {
                    (
                        ToastKind::Error,
                        tr!(
                            "actions.endGroup.partial",
                            error = first_error,
                            count = failed.len()
                        ),
                        5000,
                    )
                }
            }
            "end" => {
                if failed.is_empty() {
                    (
                        ToastKind::Success,
                        tr!("actions.end.done", name = name),
                        2500,
                    )
                } else {
                    (ToastKind::Error, first_error, 5000)
                }
            }
            "endTree" => {
                let report = &results
                    .first()
                    .map(|r| r["value"].clone())
                    .unwrap_or_default();
                if !failed.is_empty() {
                    (ToastKind::Error, first_error, 5000)
                } else if number(report, "failed") > 0. {
                    (
                        ToastKind::Error,
                        tr!(
                            "actions.endTree.partial",
                            count = number(report, "terminated") as u64,
                            failed = number(report, "failed") as u64
                        ),
                        5000,
                    )
                } else {
                    (
                        ToastKind::Success,
                        tr!(
                            "actions.endTree.done",
                            count = number(report, "terminated") as u64
                        ),
                        2500,
                    )
                }
            }
            "priority" => {
                let wanted = text(value, "priority");
                if failed.len() == total {
                    (
                        ToastKind::Error,
                        format!(
                            "{}：{first_error}",
                            tr!("actions.priority.failed", name = name)
                        ),
                        5000,
                    )
                } else if results.iter().any(|r| {
                    boolean(r, "ok") && r["value"].as_str().is_some_and(|got| got != wanted)
                }) {
                    (ToastKind::Error, label("actions.priority.downgraded"), 5000)
                } else if !failed.is_empty() {
                    (
                        ToastKind::Error,
                        tr!(
                            "actions.priority.partial",
                            count = total - failed.len(),
                            failed = failed.len()
                        ),
                        5000,
                    )
                } else {
                    (
                        ToastKind::Success,
                        tr!(
                            "actions.priority.done",
                            name = name,
                            priority = label(&format!("processes.priority.{wanted}"))
                        ),
                        2500,
                    )
                }
            }
            _ => {
                let state = if boolean(value, "on") { "on" } else { "off" };
                if failed.len() == total {
                    (
                        ToastKind::Error,
                        format!(
                            "{}：{first_error}",
                            argus_core::i18n::format(
                                &format!("actions.efficiency.{state}.failed"),
                                &[("name", &name)]
                            )
                        ),
                        5000,
                    )
                } else if !failed.is_empty() {
                    (
                        ToastKind::Error,
                        argus_core::i18n::format(
                            &format!("actions.efficiency.{state}.partial"),
                            &[
                                ("count", &(total - failed.len())),
                                ("failed", &failed.len()),
                            ],
                        ),
                        5000,
                    )
                } else {
                    (
                        ToastKind::Success,
                        argus_core::i18n::format(
                            &format!("actions.efficiency.{state}.done"),
                            &[("name", &name)],
                        ),
                        2500,
                    )
                }
            }
        };
        if kind_out == ToastKind::Error && outdated {
            // Denied while the service needs an update: the notice leads to its card.
            self.toast_with(
                ToastKind::Error,
                text_out,
                10000,
                vec![(
                    label("settings.privilege.update"),
                    Rc::new(
                        |this: &mut Desktop, window: &mut Window, cx: &mut Context<Desktop>| {
                            this.locate_setting("privilege", "privilegedService", window, cx)
                        },
                    ),
                )],
            );
        } else {
            self.toast_with(kind_out, text_out, ms, vec![]);
        }
    }

    /// Opens Settings on the rule for `name`, creating one first (actions.ts, addRuleFor).
    fn add_rule_for(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        if name.is_empty() {
            return;
        }
        let rules = values(&self.data["settings"]["rules"]).to_vec();
        if let Some(rule) = rules
            .iter()
            .find(|r| text(r, "name").eq_ignore_ascii_case(name))
        {
            self.ui.focus_rule = Some((text(rule, "id").to_owned(), Instant::now()));
            self.preference("settingsSection", json!("rules"), window, cx);
            return;
        }
        let processes = values(&self.snapshot["processes"]);
        let same = processes
            .iter()
            .filter(|p| text(p, "name").eq_ignore_ascii_case(name))
            .collect::<Vec<_>>();
        let process = same.first().cloned().cloned().unwrap_or(Value::Null);
        let broad = SHARED_HOSTS.contains(&name.to_ascii_lowercase().as_str())
            || same
                .iter()
                .map(|p| text(p, "path").to_ascii_lowercase())
                .collect::<HashSet<_>>()
                .len()
                > 1;
        let display = name_of(&same).unwrap_or_else(|| name.to_owned());
        let name = name.to_owned();
        let create = move |this: &mut Desktop, window: &mut Window, cx: &mut Context<Desktop>| {
            let priority = process["priorityClass"]
                .as_str()
                .filter(|p| *p != "normal" && PRIORITIES.contains(p))
                .map(|p| json!(p))
                .unwrap_or(Value::Null);
            let id = format!(
                "native-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
            );
            let mut rules = values(&this.data["settings"]["rules"]).to_vec();
            rules.push(
                json!({"id":id,"name":name,"enabled":true,"priority":priority,
                "efficiency":if boolean(&process,"efficiency") {json!(true)} else {Value::Null}}),
            );
            this.request(
                "set_rules",
                json!({"rules":rules}),
                Some("settings"),
                window,
                cx,
            );
            this.ui.focus_rule = Some((id, Instant::now()));
            this.preference("settingsSection", json!("rules"), window, cx);
        };
        if broad {
            self.ask(
                label("actions.ruleScope.title"),
                tr!("actions.ruleScope.body", name = display),
                label("actions.ruleScope.confirm"),
                false,
                create,
                window,
                cx,
            );
        } else {
            create(self, window, cx);
        }
    }

    /// Restarts Explorer with a pending notice, then says how it went (explorer.ts).
    fn restart_explorer(&mut self, shell: Value, window: &mut Window, cx: &mut Context<Self>) {
        if self.ui.busy.contains("explorer") {
            return;
        }
        self.ui.busy.insert("explorer".into());
        if boolean(&shell, "locker") {
            self.ui
                .errors
                .insert("explorerLocker".into(), String::new());
        }
        let pending = self.toast_with(
            ToastKind::Info,
            label("tasks.explorer.restarting"),
            60000,
            vec![],
        );
        self.ui
            .errors
            .insert("explorerToast".into(), pending.to_string());
        // A fixture shows the restart under way but never asks for it; a
        // test control (`resolve`) answers in its place.
        if self.options.fixture.is_some() {
            return;
        }
        self.request(
            "restart_explorer",
            json!({"pid":shell["pid"],"createTime":shell["createTime"]}),
            None,
            window,
            cx,
        );
    }

    fn explorer_finished(&mut self, error: Option<&Value>) {
        self.ui.busy.remove("explorer");
        self.ui.errors.remove("explorerLocker");
        if let Some(id) = self
            .ui
            .errors
            .remove("explorerToast")
            .and_then(|id| id.parse::<u64>().ok())
        {
            self.dismiss_toast(id);
        }
        let Some(error) = error else {
            self.notify_toast(ToastKind::Success, label("tasks.explorer.done"));
            return;
        };
        let kind = text(error, "kind");
        let key = if matches!(
            kind,
            "stale" | "unsupported" | "permission" | "prepare" | "shutdown" | "restore" | "busy"
        ) {
            format!("tasks.explorer.error.{kind}")
        } else {
            "tasks.explorer.error.prepare".into()
        };
        let mut message = label(&key);
        if let Some(code) = hex_code(&error["code"]) {
            message = format!("{message} {code}");
        }
        let actions: Vec<(String, Handler)> = if boolean(error, "shellMissing") {
            vec![(
                label("tasks.explorer.recover"),
                Rc::new(
                    |this: &mut Desktop, window: &mut Window, cx: &mut Context<Desktop>| {
                        this.open_dialog(
                            "task",
                            json!({"command":"%WINDIR%\\explorer.exe"}),
                            window,
                            cx,
                        )
                    },
                ),
            )]
        } else {
            vec![]
        };
        self.toast_with(ToastKind::Error, message, 15000, actions);
    }

    /// Runs the new task's command (NewTaskDialog.tsx, run).
    fn run_task(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(modal) = self.modal.as_mut().filter(|m| m.kind == "task") else {
            return;
        };
        if self.ui.busy.contains("task") || self.ui.busy.contains("pick") {
            return;
        }
        let command = self
            .forms
            .get("value")
            .map(|v| v.read(cx).value().to_string())
            .unwrap_or_default();
        if !valid_task(&command) {
            modal.args["failure"] = json!({"kind":"invalid"});
            window.refresh();
            return;
        }
        modal.args["failure"] = Value::Null;
        let elevated = boolean(&modal.args, "elevated");
        self.ui.busy.insert("task".into());
        self.request(
            "run_task",
            json!({"command":command.trim(),"elevated":elevated}),
            None,
            window,
            cx,
        );
        window.refresh();
        cx.notify();
    }

    fn task_dialog(&self, dialog: Dialog, modal: &Modal, cx: &mut Context<Self>) -> Dialog {
        let p = self.palette;
        let id = modal.id;
        let busy = self.ui.busy.contains("task");
        let pending = busy || self.ui.busy.contains("pick");
        let elevated = boolean(&modal.args, "elevated");
        let command = self
            .forms
            .get("value")
            .map(|v| v.read(cx).value().to_string())
            .unwrap_or_default();
        let failure = modal.args["failure"].clone();
        let failure_kind = text(&failure, "kind").to_owned();
        let shown_error = !failure_kind.is_empty() && failure_kind != "cancelled";
        let feedback = if !failure_kind.is_empty() {
            let key = format!("tasks.error.{failure_kind}");
            let message = label(&key);
            Some((
                if message == key {
                    label("tasks.error.os")
                } else {
                    message
                },
                if failure_kind == "cancelled" {
                    None
                } else {
                    hex_code(&failure["code"])
                },
            ))
        } else {
            None
        };
        dialog
            .child(
                self.measure(id, 0).child(
                    div()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .flex()
                                .items_start()
                                .gap(px(14.))
                                .pt(px(26.))
                                .px(px(24.))
                                .child(
                                    div()
                                        .size(px(44.))
                                        .flex_shrink_0()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .border_1()
                                        .border_color(p.stroke)
                                        .rounded(px(10.))
                                        .bg(p.layer_strong)
                                        .child(icon("RunTaskIcon", p.accent).size(px(24.))),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .flex()
                                        .flex_col()
                                        .gap(px(5.))
                                        .child(self.dialog_title(modal.title.clone()))
                                        .child(
                                            div()
                                                .text_size(theme::font(12.))
                                                .line_height(relative(1.6))
                                                .text_color(p.muted)
                                                .child(modal.body.clone()),
                                        ),
                                )
                                .child(
                                    self.btn(
                                        format!("modal-{id}-x"),
                                        "",
                                        Some("DismissIcon"),
                                        Tone::Plain,
                                    )
                                    .mt(px(-8.))
                                    .mr(px(-8.))
                                    .accessibility_label(label("common.close"))
                                    .inert(pending)
                                    .on_click(cx.listener(
                                        |this, _, window, cx| this.close_overlay(window, cx),
                                    )),
                                ),
                        )
                        .child(
                            div()
                                .pt(px(22.))
                                .px(px(24.))
                                .pb(px(8.))
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .mb(px(8.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(label("tasks.command")),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .gap(px(8.))
                                        .child(div().flex_1().min_w_0().h(px(36.)).when_some(
                                            self.forms.get("value"),
                                            |d, input| {
                                                d.child(
                                                    // The original's command box has no
                                                    // face of its own (`.new-task-input`).
                                                    Input::new(input)
                                                        .appearance(false)
                                                        .h(px(36.))
                                                        .px(px(11.))
                                                        .font_family(theme::MONO_FONT)
                                                        .text_size(theme::font(13.))
                                                        .disabled(pending),
                                                )
                                            },
                                        ))
                                        .child(
                                            self.btn(
                                                format!("modal-{id}-browse"),
                                                label("tasks.browse"),
                                                Some("FolderIcon"),
                                                Tone::Plain,
                                            )
                                            .h(px(36.))
                                            .inert(pending)
                                            .on_click(
                                                cx.listener(|this, _, window, cx| {
                                                    if this.options.fixture.is_some() {
                                                        return;
                                                    }
                                                    this.ui.busy.insert("pick".into());
                                                    this.request(
                                                        "pick_path",
                                                        json!({"folder":false,"task":true}),
                                                        None,
                                                        window,
                                                        cx,
                                                    );
                                                }),
                                            ),
                                        ),
                                )
                                .child(
                                    div()
                                        .id(("task-elevated", id))
                                        .mt(px(20.))
                                        .flex()
                                        .items_start()
                                        .gap(px(10.))
                                        .cursor_pointer()
                                        .when(!pending, |d| {
                                            d.on_click(cx.listener(move |this, _, window, cx| {
                                                if let Some(modal) = &mut this.modal
                                                    && modal.id == id
                                                {
                                                    let on = !boolean(&modal.args, "elevated");
                                                    modal.args["elevated"] = json!(on);
                                                    modal.args["failure"] = Value::Null;
                                                }
                                                window.refresh();
                                                cx.notify();
                                            }))
                                        })
                                        .child(
                                            div()
                                                .mt(px(2.))
                                                .size(px(17.))
                                                .flex_shrink_0()
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .rounded(px(4.))
                                                .border_1()
                                                .border_color(if elevated {
                                                    p.accent
                                                } else {
                                                    p.muted
                                                })
                                                .bg(if elevated {
                                                    p.accent
                                                } else {
                                                    p.background.opacity(0.)
                                                })
                                                .when(elevated, |d| {
                                                    d.child(
                                                        icon("CheckIcon", p.on_accent)
                                                            .size(px(12.)),
                                                    )
                                                }),
                                        )
                                        .child(
                                            div()
                                                .flex()
                                                .flex_col()
                                                .child(
                                                    div()
                                                        .flex()
                                                        .items_center()
                                                        .gap(px(8.))
                                                        .child(label("tasks.administrator"))
                                                        .child(
                                                            icon("ShieldIcon", p.muted)
                                                                .size(px(14.)),
                                                        ),
                                                )
                                                .child(
                                                    div()
                                                        .mt(px(3.))
                                                        .min_h(px(36.))
                                                        .text_size(theme::font(12.))
                                                        .line_height(relative(1.5))
                                                        .text_color(p.muted)
                                                        .child(label(if elevated {
                                                            "tasks.administratorHint"
                                                        } else {
                                                            "tasks.standardHint"
                                                        })),
                                                ),
                                        ),
                                )
                                .child(
                                    div()
                                        .min_h(px(62.))
                                        .pt(px(8.))
                                        .text_size(theme::font(12.))
                                        .line_height(relative(1.5))
                                        .text_color(if shown_error { p.danger } else { p.muted })
                                        .flex()
                                        .flex_col()
                                        .when_some(feedback, |d, (message, code)| {
                                            d.child(message).when_some(code, |d, code| {
                                                d.child(
                                                    div().text_size(theme::font(11.)).child(code),
                                                )
                                            })
                                        })
                                        .when(failure_kind.is_empty() && busy, |d| {
                                            d.child(label("tasks.waiting"))
                                        }),
                                ),
                        ),
                ),
            )
            .footer(
                self.measure(id, 1).child(
                    self.dialog_footer()
                        .child(
                            self.btn(
                                format!("modal-{id}-cancel"),
                                label("common.cancel"),
                                None,
                                Tone::Plain,
                            )
                            .min_w(px(96.))
                            .inert(pending)
                            .on_click(
                                cx.listener(|this, _, window, cx| this.close_overlay(window, cx)),
                            ),
                        )
                        .child(
                            self.btn(
                                format!("modal-{id}-run"),
                                label(if busy { "tasks.running" } else { "tasks.run" }),
                                None,
                                Tone::Accent,
                            )
                            .min_w(px(96.))
                            .track_focus(&self.primary_focus(id, cx))
                            .inert(pending || !valid_task(&command))
                            .on_click(cx.listener(|this, _, window, cx| this.run_task(window, cx))),
                        ),
                ),
            )
    }

    /// Checks the chosen paths for lockers (FileLockDialog.tsx, search).
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
    fn recheck_lockers(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

    /// "Who is using this file?" (FileLockDialog.tsx): the chosen paths or a
    /// drop zone, the pickers, then the search, its error or its result.
    fn filelock_dialog(
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
                            .font_weight(FontWeight::SEMIBOLD)
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
                self.measure(id, 0).child(
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
    fn lock_report(&self, report: &Value, cx: &mut Context<Self>) -> AnyElement {
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
    fn locker_row(
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
        // The original's source has an ideographic space here, but its JSX
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
                .tooltip(|window, cx| {
                    gpui_kit::component::tooltip::Tooltip::new(label("fileLock.showProcess"))
                        .build(window, cx)
                })
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

    fn pick_lock_path(&mut self, folder: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.options.fixture.is_some() {
            return;
        }
        self.request("pick_path", json!({"folder":folder}), None, window, cx);
    }

    /// Ends a locker after asking, then checks again (actions.ts, endProcessByPid).
    fn end_locker(&mut self, locker: Value, window: &mut Window, cx: &mut Context<Self>) {
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

    fn note_dialog(&self, dialog: Dialog, modal: &Modal, cx: &mut Context<Self>) -> Dialog {
        let p = self.palette;
        let id = modal.id;
        dialog
            .child(
                self.measure(id, 0).child(
                    div()
                        .pt(px(22.))
                        .px(px(24.))
                        .pb(px(20.))
                        .flex()
                        .flex_col()
                        .child(self.dialog_title(modal.title.clone()))
                        .child(
                            // `.note-target`: one line, 4 px into the title's
                            // 10 px margin and 16 px above the field.
                            div()
                                .mt(px(6.))
                                .mb(px(16.))
                                .truncate()
                                .text_size(theme::font(13.))
                                .text_color(p.muted)
                                .child(modal.body.clone()),
                        )
                        .when_some(self.forms.get("value"), |d, input| {
                            d.child(Input::new(input).text_size(theme::font(13.)))
                        }),
                ),
            )
            .footer(
                self.measure(id, 1).child(
                    self.dialog_footer()
                        .child(
                            self.btn(
                                format!("modal-{id}-save"),
                                label("common.marks.save"),
                                None,
                                Tone::Accent,
                            )
                            .min_w(px(96.))
                            .track_focus(&self.primary_focus(id, cx))
                            .on_click(
                                cx.listener(|this, _, window, cx| this.confirm_dialog(window, cx)),
                            ),
                        )
                        .child(
                            self.btn(
                                format!("modal-{id}-cancel"),
                                label("common.cancel"),
                                None,
                                Tone::Plain,
                            )
                            .min_w(px(96.))
                            .on_click(
                                cx.listener(|this, _, window, cx| this.close_overlay(window, cx)),
                            ),
                        ),
                ),
            )
    }

    /// A long text in a bordered monospaced box (Settings.tsx, DocumentDialog).
    fn document_dialog(&self, dialog: Dialog, modal: &Modal, cx: &mut Context<Self>) -> Dialog {
        let p = self.palette;
        let id = modal.id;
        let document = scrollbar::fresh_handle("document", id);
        dialog
            .child(
                self.measure(id, 0).child(
                    div()
                        .pt(px(22.))
                        .px(px(24.))
                        .pb(px(20.))
                        .flex()
                        .flex_col()
                        .gap(px(10.))
                        .child(self.dialog_title(modal.title.clone()))
                        .child(
                            div()
                                .id(("document", id))
                                .track_scroll(&document)
                                .max_h(px(560.))
                                .overflow_y_scroll()
                                .p(px(12.))
                                // The gutter is kept (`scrollbar-gutter: stable`).
                                .pr(px(12. + scrollbar::SIZE))
                                .border_1()
                                .border_color(p.stroke)
                                .rounded(px(6.))
                                .bg(p.layer)
                                .font_family(theme::MONO_FONT)
                                .text_size(theme::font(12.))
                                .line_height(relative(1.5))
                                .text_color(p.muted)
                                .child(modal.body.clone())
                                .child(
                                    scrollbar::scrollbar("document-scrollbar", &document, p.dark)
                                        .border(1.),
                                ),
                        ),
                ),
            )
            .footer(
                self.measure(id, 1)
                    .child(self.dialog_footer().child(self.close_button(id, cx))),
            )
    }

    /// A determinate (`value`) or sweeping progress bar (ProgressBar.tsx).
    pub(in crate::frontend) fn progress_bar(&self, value: Option<f32>, _cx: &App) -> AnyElement {
        let p = self.palette;
        let track = div()
            .relative()
            .w_full()
            .h(px(3.))
            .rounded(px(2.))
            .overflow_hidden()
            .bg(p.accent.opacity(0.16));
        match value {
            Some(value) => track
                .child(
                    div()
                        .absolute()
                        .left_0()
                        .top_0()
                        .bottom_0()
                        .w(relative(value.clamp(0., 1.)))
                        .rounded(px(2.))
                        .bg(p.accent),
                )
                .into_any_element(),
            None => track
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .w(relative(0.4))
                        .rounded(px(2.))
                        .bg(p.accent)
                        .with_animation(
                            "progress-sweep",
                            Animation::new(Duration::from_millis(1400))
                                .repeat()
                                .with_easing(theme::ease_move),
                            |bar, delta| bar.left(relative(-0.4 + 1.4 * delta)),
                        ),
                )
                .into_any_element(),
        }
    }

    /// A process's own icon, or the generic app glyph.
    pub(in crate::frontend) fn process_icon(
        &self,
        process: Option<&Value>,
        size: f32,
        cx: &App,
    ) -> AnyElement {
        let id = process.and_then(|p| p["icon"].as_u64()).map(|id| id as u32);
        let image = id.and_then(|id| {
            let mut cache = self.ui.icons.borrow_mut();
            if let Some(image) = cache.get(&id) {
                return Some(image.clone());
            }
            let png = self.host.state::<AppState>().meta.icon_png(id)?;
            let image = Arc::new(Image::from_bytes(ImageFormat::Png, png.to_vec()));
            cache.insert(id, image.clone());
            Some(image)
        });
        let _ = cx;
        match image {
            Some(image) => img(image).size(px(size)).flex_shrink_0().into_any_element(),
            None => icon("AppIcon", self.palette.muted)
                .size(px(size))
                .flex_shrink_0()
                .into_any_element(),
        }
    }

    pub(in crate::frontend) fn command_completed(
        &mut self,
        command: &str,
        value: &Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match command {
            "complete_onboarding" => self.onboarding_saved(value, window, cx),
            "batch" => self.batch_done(value),
            "pick_path" => {
                self.ui.busy.remove("pick");
                let Some(path) = value.as_str() else {
                    return;
                };
                match self.modal.as_ref().map(|m| m.kind.as_str()) {
                    Some("task") => {
                        if let Some(input) = self.forms.get("value") {
                            let quoted = format!("\"{path}\"");
                            input.update(cx, |state, cx| {
                                state.set_value(quoted, window, cx);
                                state.focus(window, cx);
                            });
                        }
                        if let Some(modal) = &mut self.modal {
                            modal.args["failure"] = Value::Null;
                        }
                    }
                    Some("filelock") => self.search_lockers(vec![path.to_owned()], window, cx),
                    _ => {}
                }
            }
            "run_task" => {
                self.ui.busy.remove("task");
                self.close_overlay(window, cx);
                self.notify_toast(ToastKind::Success, label("tasks.started"));
            }
            "restart_explorer" => {
                let from_lockers = self.ui.errors.contains_key("explorerLocker");
                self.explorer_finished(None);
                if from_lockers {
                    self.recheck_lockers(window, cx);
                }
            }
            "set_theme" => {
                let saved = self.host.state::<crate::settings::SettingsStore>().get();
                self.set_theme(crate::platform::is_dark(saved.theme), window, cx);
            }
            "privileged_service_status" => {
                self.data.insert("privilege".into(), value.clone());
            }
            "kill_process" => {
                if let Some(name) = self.ui.errors.remove("lockerEnd") {
                    self.notify_toast_for(
                        ToastKind::Success,
                        tr!("actions.end.done", name = name),
                        2500,
                    );
                    self.recheck_lockers(window, cx);
                }
            }
            "control_service"
            | "set_service_start_type"
            | "set_startup_enabled"
            | "close_connection" => self.poll_page(window, cx),
            _ => self.settings_completed(command, value, window, cx),
        }
    }

    /// A failed command: shown where it belongs (a card, the open dialog), or
    /// as a notice. Returns whether it was shown in place.
    pub(in crate::frontend) fn command_failed(
        &mut self,
        command: &str,
        args: &Value,
        error: &Value,
        message: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match command {
            "run_task" => {
                self.ui.busy.remove("task");
                if let Some(input) = self.forms.get("value") {
                    input.update(cx, |state, cx| state.focus(window, cx));
                }
                if let Some(modal) = &mut self.modal
                    && modal.kind == "task"
                {
                    modal.args["failure"] = if error.is_object() {
                        error.clone()
                    } else {
                        json!({"kind":"os"})
                    };
                }
            }
            "pick_path" => {
                self.ui.busy.remove("pick");
                self.notify_toast(ToastKind::Error, message);
            }
            "find_lockers" => {
                let text_error = if text(error, "kind") == "notFound" {
                    label("fileLock.error.notFound")
                } else if text(error, "kind") == "os" {
                    message
                } else {
                    tr!("fileLock.error.failed", error = message)
                };
                if let Some(modal) = &mut self.modal
                    && modal.kind == "filelock"
                {
                    modal.error = Some(text_error);
                }
            }
            "restart_explorer" => self.explorer_finished(Some(error)),
            "kill_process" if boolean(args, "locker") => {
                self.ui.errors.remove("lockerEnd");
                self.notify_toast(ToastKind::Error, message);
            }
            _ => {
                if !self.settings_failed(command, args, error, &message, window, cx) {
                    self.notify_toast(ToastKind::Error, message);
                }
            }
        }
        window.refresh();
    }

    // ---- Toasts (Overlays.tsx, lib/ui.ts, toast-clock.ts) -------------------

    /// Coming back from the tray or a minimized window: when it sampled less
    /// often meanwhile, says so once, with the way to that setting
    /// (lib/background.ts, noticeRestored).
    pub(in crate::frontend) fn restored(&mut self, value: &Value) {
        let tray = text(value, "from") == "tray";
        let quiet = if tray {
            "notice.trayRate"
        } else {
            "notice.minimizedRate"
        };
        if self.prefs.get::<bool>(quiet, false) || self.modal.is_some() {
            return;
        }
        let away = if tray { "tray" } else { "minimized" };
        let field = if tray {
            "trayInterval"
        } else {
            "minimizedInterval"
        };
        let hidden = number(&self.data["settings"], field);
        let shown = self.interval as f64;
        if hidden <= shown || number(value, "awayMs") <= shown {
            return;
        }
        self.toast_with(
            ToastKind::Info,
            argus_core::i18n::format(
                &format!("shell.background.notice.{away}"),
                &[("from", &(shown / 1000.)), ("to", &(hidden / 1000.))],
            ),
            10000,
            vec![
                (
                    label("shell.background.notice.adjust"),
                    Rc::new(
                        move |this: &mut Desktop,
                              window: &mut Window,
                              cx: &mut Context<Desktop>| {
                            this.locate_setting("data", field, window, cx)
                        },
                    ),
                ),
                (
                    label("shell.background.notice.quiet"),
                    Rc::new(
                        move |this: &mut Desktop, _: &mut Window, _: &mut Context<Desktop>| {
                            this.prefs.set(quiet, true)
                        },
                    ),
                ),
            ],
        );
    }

    /// A preference write finished: when saving starts to fail, says so once
    /// (App.tsx, `preferenceSaveFailed`).
    pub(in crate::frontend) fn preferences_written(
        &mut self,
        saved: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.prefs.record(saved) {
            self.notify_toast(ToastKind::Error, label("settings.appearance.saveFailed"));
            window.refresh();
            cx.notify();
        }
    }

    /// Shows a notice for five seconds.
    pub(in crate::frontend) fn notify_toast(&mut self, kind: ToastKind, text: impl Into<String>) {
        self.toast_with(kind, text, 5000, vec![]);
    }

    /// Shows a notice for `ms`.
    pub(in crate::frontend) fn notify_toast_for(
        &mut self,
        kind: ToastKind,
        text: impl Into<String>,
        ms: u64,
    ) {
        self.toast_with(kind, text, ms, vec![]);
    }

    /// Shows a notice for `ms` with buttons under its text; at most four at a
    /// time, the oldest giving way. Returns its id.
    pub(in crate::frontend) fn toast_with(
        &mut self,
        kind: ToastKind,
        text: impl Into<String>,
        ms: u64,
        actions: Vec<(String, Handler)>,
    ) -> u64 {
        self.ui.toast_seq += 1;
        let id = self.ui.toast_seq;
        let now = Instant::now();
        let live = self
            .ui
            .toasts
            .iter()
            .filter(|t| t.leaving.is_none())
            .count();
        if live >= 4 {
            let mut excess = live - 3;
            for toast in self.ui.toasts.iter_mut().filter(|t| t.leaving.is_none()) {
                if excess == 0 {
                    break;
                }
                toast.leaving = Some(now);
                excess -= 1;
            }
        }
        self.ui.toasts.push(Toast {
            id,
            kind,
            text: text.into(),
            actions,
            born: now,
            expires: Some(now + Duration::from_millis(ms)),
            hovered: false,
            leaving: None,
        });
        id
    }

    pub(in crate::frontend) fn dismiss_toast(&mut self, id: u64) {
        if let Some(toast) = self
            .ui
            .toasts
            .iter_mut()
            .find(|t| t.id == id && t.leaving.is_none())
        {
            toast.leaving = Some(Instant::now());
        }
    }

    /// The quarter-second clock: notices that are due leave, gone ones are
    /// dropped, and motion follows a system change when not set here.
    pub(in crate::frontend) fn tick_ui(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let now = Instant::now();
        let mut changed = false;
        for toast in &mut self.ui.toasts {
            if toast.leaving.is_none() && toast.expires.is_some_and(|at| at <= now) {
                toast.leaving = Some(now);
                changed = true;
            }
        }
        let before = self.ui.toasts.len();
        self.ui.toasts.retain(|t| {
            t.leaving
                .is_none_or(|at| now.duration_since(at) < Duration::from_millis(170))
        });
        changed |= before != self.ui.toasts.len();
        self.ui.ticks = self.ui.ticks.wrapping_add(1);
        if self.ui.ticks.is_multiple_of(4) {
            let system = crate::platform::system_motion();
            if self.ui.system_motion.is_some_and(|before| before != system)
                && self.prefs.get::<Option<bool>>("motion.ui", None).is_none()
            {
                self.apply_motion(cx);
                changed = true;
            }
            self.ui.system_motion = Some(system);
        }
        if self.ui.ticks.is_multiple_of(4) {
            self.announce_update(window, cx);
        }
        changed |= self.announce_administrator();
        changed |= self.tick_hotkey(window, cx);
        changed |= self.tick_settings();
        if changed {
            window.refresh();
            cx.notify();
        }
    }

    fn toast_layer(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.ui.toasts.is_empty() {
            return None;
        }
        let p = self.palette;
        let reduce = cx.reduce_motion();
        let viewport = window.viewport_size();
        let width = px(380.).min(viewport.width - px(40.));
        let mut animating = false;
        let stack = div()
            .absolute()
            .right(px(20.))
            .bottom(px(40.))
            .w(width)
            .flex()
            .flex_col()
            .gap(px(8.))
            .children(self.ui.toasts.iter().map(|toast| {
                let id = toast.id;
                let enter = if reduce {
                    1.
                } else {
                    (toast.born.elapsed().as_secs_f32() / 0.25).min(1.)
                };
                let leave = toast.leaving.map(|at| {
                    if reduce {
                        1.
                    } else {
                        (at.elapsed().as_secs_f32() / 0.167).min(1.)
                    }
                });
                if enter < 1. || leave.is_some_and(|l| l < 1.) {
                    animating = true;
                }
                let entered = theme::ease_out(enter);
                let left = leave.map(theme::ease_in).unwrap_or(0.);
                let (glyph, color) = match toast.kind {
                    ToastKind::Error => ("WarningIcon", p.danger),
                    ToastKind::Info => ("InfoIcon", p.accent),
                    ToastKind::Success => ("CheckIcon", p.accent),
                };
                div()
                    .id(("toast", id))
                    .occlude()
                    .relative()
                    .top(px((1. - entered) * 16.))
                    .left(px(left * 24.))
                    .opacity(entered * (1. - left))
                    .flex()
                    .items_start()
                    .gap(px(10.))
                    .pl(px(12.))
                    .pr(px(8.))
                    .py(px(10.))
                    .border_1()
                    .border_color(p.stroke_strong)
                    .rounded(px(8.))
                    // No shadow, as the original's toasts (`.toast`).
                    .bg(widgets::solid(p.flyout))
                    .text_color(p.ink)
                    .text_size(theme::font(13.))
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        if let Some(toast) = this.ui.toasts.iter_mut().find(|t| t.id == id) {
                            toast.hovered = *hovered;
                            // Held while the pointer is on it; it lingers 2.5 s after.
                            toast.expires = if *hovered {
                                None
                            } else {
                                Some(Instant::now() + Duration::from_millis(2500))
                            };
                        }
                        cx.notify();
                    }))
                    .child(div().mt(px(2.)).child(icon(glyph, color)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(toast.text.clone())
                            .when(!toast.actions.is_empty(), |d| {
                                d.child(
                                    div().flex().flex_wrap().gap(px(6.)).mt(px(8.)).children(
                                        toast.actions.iter().enumerate().map(
                                            |(index, (name, run))| {
                                                let run = run.clone();
                                                self.btn_small(
                                                    format!("toast-{id}-{index}"),
                                                    name.clone(),
                                                    None,
                                                    Tone::Plain,
                                                )
                                                .on_click(cx.listener(
                                                    move |this, _, window, cx| {
                                                        if this.ui.toasts.iter().any(|t| {
                                                            t.id == id && t.leaving.is_some()
                                                        }) {
                                                            return;
                                                        }
                                                        this.dismiss_toast(id);
                                                        run(this, window, cx);
                                                        cx.notify();
                                                    },
                                                ))
                                            },
                                        ),
                                    ),
                                )
                            }),
                    )
                    .child(
                        self.btn_small(
                            format!("toast-{id}-close"),
                            "",
                            Some("DismissIcon"),
                            Tone::Ghost,
                        )
                        .accessibility_label(label("common.close"))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.dismiss_toast(id);
                            cx.notify();
                        })),
                    )
            }));
        if animating {
            window.request_animation_frame();
        }
        Some(
            deferred(
                anchored()
                    .position(point(px(0.), px(0.)))
                    .child(div().w(viewport.width).h(viewport.height).child(stack)),
            )
            .with_priority(50)
            .into_any_element(),
        )
    }

    pub(in crate::frontend) fn render_overlays(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let mut elements = vec![];
        elements.extend(self.menu_overlay(window, cx));
        elements.extend(self.select_overlay(window, cx));
        elements.extend(self.toast_layer(window, cx));
        elements
    }
}

/// The executable name as the processes report it, else as given.
fn name_of(same: &[&Value]) -> Option<String> {
    same.first().map(|p| text(p, "name").to_owned())
}

/// When the dim behind the dialogs (`.scrim`) came and started to go.
#[derive(Default)]
pub(super) struct ScrimState {
    pub shown: Cell<Option<Instant>>,
    pub leaving: Cell<Option<Instant>>,
}

/// The dim behind dialogs, a layer of its own over the desktop and its chart
/// layer, so the moving curves dim with everything else; the caption buttons
/// stay above it. It fades in over 167 ms when the first dialog shows and out
/// (accelerating) with the last.
pub(super) struct Scrim {
    pub state: Rc<ScrimState>,
}

impl Render for Scrim {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let reduce = cx.reduce_motion();
        let progress = |since: Instant| {
            if reduce {
                1.
            } else {
                (since.elapsed().as_secs_f32() / 0.167).min(1.)
            }
        };
        let level = match (self.state.shown.get(), self.state.leaving.get()) {
            (_, Some(leaving)) => 1. - theme::ease_in(progress(leaving)),
            (Some(shown), None) => progress(shown),
            (None, None) => 0.,
        };
        let settled = match (self.state.shown.get(), self.state.leaving.get()) {
            (_, Some(leaving)) => progress(leaving) >= 1.,
            (Some(shown), None) => progress(shown) >= 1.,
            (None, None) => true,
        };
        if !settled {
            window.request_animation_frame();
        }
        let color = theme::token("scrim", Palette::current(cx).dark).opacity(level);
        // The caption buttons stay above the dim, as the original's (`.caption`
        // over `.scrim`): the toolbar row stops short of them.
        let toolbar = theme::dimension("toolbarHeight");
        let captions = gpui_kit::component::TITLE_BAR_HEIGHT * 3.;
        let dim = || div().absolute().bg(color);
        div().absolute().inset_0().when(level > 0., |d| {
            d.child(dim().top_0().left_0().right(captions).h(toolbar))
                .child(dim().top(toolbar).left_0().right_0().bottom_0())
        })
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
