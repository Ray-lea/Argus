use super::*;
mod file_lock;
mod process_actions;
use gpui_kit::base::ElementExt;
use gpui_kit::component::WindowExt;
use gpui_kit::component::dialog::{Dialog, DialogEntrance};
use widgets::{ButtonExt, Field, Handler, Toast, ToastKind, Tone};

/// What confirming a generic confirmation does.
#[derive(Clone)]
pub(super) struct Ask {
    pub label: String,
    pub danger: bool,
    pub then: Handler,
}

#[cfg(test)]
mod frame_tests {
    use super::{DialogFrame, DialogLayout, part_height_changed};
    use gpui_kit::gpui::{ScrollHandle, px};
    use std::time::{Duration, Instant};

    fn frame(now: Instant) -> DialogFrame {
        DialogFrame {
            top: px(0.),
            viewport: px(-1.),
            from: px(0.),
            to: px(0.),
            since: now,
            clip: None,
            body_height: px(0.),
            scroll: ScrollHandle::new(),
        }
    }

    #[test]
    fn fractional_scale_roundoff_does_not_restart_dialog_layout() {
        for scale in [1., 1.25, 1.5, 1.75, 2.25, 3.] {
            let before = px(435.7143);
            let after = px(f32::from(before) + 0.000_06);
            assert!(!part_height_changed(before, after, scale));
            assert!(!part_height_changed(after, before, scale));
            assert!(part_height_changed(
                before,
                before + px(0.25 / scale),
                scale
            ));
            assert!(part_height_changed(
                before,
                before - px(0.25 / scale),
                scale
            ));
        }
    }

    #[test]
    fn asynchronous_release_content_stays_centered_while_growing_and_shrinking() {
        let now = Instant::now();
        let motion = Duration::from_millis(200);
        let layout = DialogLayout {
            viewport: px(900.),
            edge: px(32.),
            limit: px(868.),
            centered: true,
        };
        let mut frame = frame(now);
        frame.place(px(240.), layout, now, motion);
        assert_eq!(frame.top, px(330.));
        frame.place(px(1800.), layout, now, motion);
        for ms in [0, 50, 100, 200] {
            let (height, _) =
                frame.place(px(1800.), layout, now + Duration::from_millis(ms), motion);
            assert!((f32::from(frame.top + height / 2.) - 450.).abs() < 0.01);
        }
        assert_eq!(frame.top, px(32.));
        assert_eq!(frame.to, px(836.));
        let (height, _) = frame.place(
            px(240.),
            layout,
            now + Duration::from_secs(1),
            Duration::ZERO,
        );
        assert_eq!(frame.top + height / 2., px(450.));
    }

    #[test]
    fn forms_keep_their_top_anchor_and_resize_recenters_them() {
        let now = Instant::now();
        let layout = DialogLayout {
            viewport: px(900.),
            edge: px(32.),
            limit: px(868.),
            centered: false,
        };
        let mut frame = frame(now);
        frame.place(px(240.), layout, now, Duration::ZERO);
        frame.place(px(1800.), layout, now, Duration::ZERO);
        assert_eq!(frame.top, px(330.));
        assert_eq!(frame.to, px(538.));
        frame.place(
            px(240.),
            DialogLayout {
                viewport: px(800.),
                ..layout
            },
            now,
            Duration::ZERO,
        );
        assert_eq!(frame.top, px(280.));
    }
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

/// Where a shown dialog sits and how tall it is drawn (`Desktop::surface`).
/// Reader dialogs stay centred as content arrives. Forms keep their initial
/// top edge while easing to the new height. Their bodies scroll at the limit.
#[derive(Clone)]
pub(super) struct DialogFrame {
    /// The top edge in the window.
    top: Pixels,
    /// The viewport height `top` was centred in: a resized window centres it again.
    viewport: Pixels,
    /// The drawn height eases from `from` to `to`, starting at `since`.
    from: Pixels,
    to: Pixels,
    since: Instant,
    /// What the body is held to so that it cannot scroll: the room the drawn
    /// height leaves it. `None` once its content is taller than the dialog
    /// can grow and the height has settled; then the body scrolls.
    clip: Option<Pixels>,
    body_height: Pixels,
    scroll: ScrollHandle,
}

#[derive(Clone, Copy)]
struct DialogLayout {
    viewport: Pixels,
    edge: Pixels,
    limit: Pixels,
    centered: bool,
}

impl DialogFrame {
    /// The drawn height at `now`, easing over `length` on the Fluent move curve.
    fn height(&self, now: Instant, length: Duration) -> Pixels {
        let t = now.saturating_duration_since(self.since).as_secs_f32() / length.as_secs_f32();
        if t >= 1. || length.is_zero() {
            self.to
        } else {
            self.from + (self.to - self.from) * theme::ease_move(t)
        }
    }

    fn place(
        &mut self,
        natural: Pixels,
        layout: DialogLayout,
        now: Instant,
        length: Duration,
    ) -> (Pixels, Pixels) {
        let DialogLayout {
            viewport,
            edge,
            limit,
            centered,
        } = layout;
        if self.viewport != viewport {
            let fits = natural.min((viewport - edge * 2.).max(px(0.))).min(limit);
            self.top = ((viewport - fits) / 2.).max(edge);
            self.viewport = viewport;
            self.from = fits;
            self.to = fits;
        }
        let cap = if centered {
            viewport - edge * 2.
        } else {
            viewport - self.top - edge
        }
        .min(limit)
        .max(px(0.));
        let target = natural.min(cap);
        if target != self.to {
            self.from = self.height(now, length);
            self.to = target;
            self.since = now;
        }
        let height = self.height(now, length);
        if centered {
            self.top = ((viewport - height) / 2.).max(edge);
        }
        (height, cap)
    }
}

/// Dialogs that open over whatever is open.
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
    /// A dialog's text box, starting with `value`; the dialog draws its
    /// placeholder (`Desktop::text_field`).
    pub(in crate::frontend) fn form(
        &mut self,
        key: &str,
        value: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let value = value.to_owned();
        self.forms.insert(
            key.into(),
            cx.new(|cx| {
                let mut state = InputState::new(window, cx);
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
        mut args: Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_menu(window, cx);
        self.close_select();
        if kind == "columns" {
            args = json!({"draft":columns::Preferences::load(&self.prefs),"category":"all","query":""});
            // The order list is laid out anew: nothing glides in from last time.
            self.ui.order_flips.clear();
            self.ui.order_drag.borrow_mut().take();
        }
        let name = text(&args, "name").to_owned();
        let members = values(&args["members"]).to_vec();
        let first = members.first().cloned().unwrap_or(Value::Null);
        let pid = first["pid"].to_string();
        // Process actions that apply at once, or ask first.
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
            "columns" => (label("processes.columns.choose"), String::new(), None),
            "visibility" => (label("processes.visibility.edit"), String::new(), None),
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
                "visibility" => {
                    self.visibility_forms(&visibility::Rules::load(&self.prefs), window, cx);
                }
                "columns" => {
                    self.form("columnsSearch", "", window, cx);
                    let input = self.forms.get("columnsSearch").unwrap().clone();
                    self.ui.input_subscriptions.push(cx.subscribe_in(
                        &input,
                        window,
                        |this, input, event, window, cx| {
                            if matches!(event, InputEvent::Change)
                                && let Some(modal) =
                                    this.modal.as_mut().filter(|modal| modal.kind == "columns")
                            {
                                modal.args["query"] = json!(input.read(cx).value().to_string());
                                window.refresh();
                                cx.notify();
                            }
                        },
                    ));
                }
                "task" => {
                    self.form("value", text(&args, "command"), window, cx);
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
        if kind == "columns"
            && let Some(input) = self.forms.get("columnsSearch")
        {
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
        // The primary control takes the focus, as the interface's buttons did.
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
        let shown = self.ui.dialog_shown.borrow().contains_key(&id);
        // A measuring (still transparent) dialog has no fade clock to finish.
        if cx.reduce_motion() || !shown {
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
        self.forget_dialog(id);
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
        let closed = self.modal.take().into_iter().chain(self.ui.below.drain(..));
        for id in closed.map(|modal| modal.id).collect::<Vec<_>>() {
            self.forget_dialog(id);
        }
    }

    /// The dialog closed itself (Escape or a press outside it).
    fn modal_closed(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        self.forget_dialog(id);
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
        if matches!(closed.kind.as_str(), "task" | "columns") {
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

    /// Wraps a dialog's footer (slot 1) to measure it.
    pub(in crate::frontend) fn measure(&self, id: u64, slot: u8) -> Div {
        let parts = self.ui.dialog_parts.clone();
        div()
            .flex()
            .flex_col()
            .on_prepaint(move |bounds, window, cx| {
                note_part(
                    &parts,
                    (id, slot),
                    bounds.top(),
                    bounds.size.height,
                    window,
                    cx,
                )
            })
    }

    /// A dialog's body (slot 0). Its content is laid out at its own height
    /// (block flow, so nothing in it shrinks) and measured, while the body is
    /// held to the room the dialog's drawn height leaves it
    /// (`DialogFrame::clip`): content that fits, or that the dialog is still
    /// easing toward, never makes it scroll.
    pub(in crate::frontend) fn dialog_body(&self, id: u64) -> Stateful<Div> {
        let parts = self.ui.dialog_parts.clone();
        let (height, clipping, scroll) = self
            .ui
            .dialog_frames
            .borrow()
            .get(&id)
            .map(|frame| {
                (
                    Some(frame.clip.unwrap_or(frame.body_height)),
                    frame.clip.is_some(),
                    frame.scroll.clone(),
                )
            })
            .unwrap_or((None, true, ScrollHandle::new()));
        let measured_scroll = scroll.clone();
        div()
            .on_children_prepainted(move |_, window, cx| {
                // Tracked scrollers store child bounds on the handle instead
                // of passing them to this listener. Skip the scrollbar overlay.
                let children = (1..measured_scroll.children_count())
                    .filter_map(|index| measured_scroll.bounds_for_item(index))
                    .collect::<Vec<_>>();
                let top = children.iter().map(|c| c.top()).reduce(|a, b| a.min(b));
                let bottom = children.iter().map(|c| c.bottom()).reduce(|a, b| a.max(b));
                if let Some((top, bottom)) = top.zip(bottom) {
                    note_part(&parts, (id, 0), top, bottom - top, window, cx);
                }
            })
            .id(("dialog-body", id))
            .relative()
            .track_scroll(&scroll)
            .when_some(height, |d, height| d.h(height))
            .when(clipping, |d| d.overflow_hidden())
            .when(!clipping, |d| d.overflow_y_scroll())
            // Bound the inner native viewport so the component's outer scroller
            // stays at rest and wheel notches use the shared GPUI glide.
            .child(scrollbar::scrollbar(
                format!("dialog-{id}-scrollbar"),
                &scroll,
                self.palette.dark,
            ))
    }

    /// Drops what was kept for a dialog that closed.
    fn forget_dialog(&self, id: u64) {
        self.ui.dialog_shown.borrow_mut().remove(&id);
        self.ui.dialog_frames.borrow_mut().remove(&id);
        let mut parts = self.ui.dialog_parts.borrow_mut();
        parts.remove(&(id, 0));
        parts.remove(&(id, 1));
    }

    pub(in crate::frontend) fn dialog_test_state(&self) -> Value {
        let Some(modal) = &self.modal else {
            return Value::Null;
        };
        let frames = self.ui.dialog_frames.borrow();
        let frame = frames.get(&modal.id);
        json!({"kind":modal.kind,"ready":frame.is_some() && self.ui.dialog_shown.borrow().contains_key(&modal.id),
            "hasError":modal.error.is_some(),
            "settled":frame.is_some_and(|frame| frame.height(Instant::now(), theme::motion_duration()) == frame.to),
            "centered":frame.is_some_and(|frame| (f32::from(frame.top+frame.to/2.-frame.viewport/2.)).abs()<0.5),
            "top":frame.map(|frame|f32::from(frame.top)),"height":frame.map(|frame|f32::from(frame.to)),
            "scrollY":frame.map(|frame|f32::from(-frame.scroll.offset().y))})
    }

    /// The common dialog surface: width, colours, no close button, Fluent
    /// fade. It opens centred in the window, as the interface's scrim centred
    /// it. Release readers retain their centre during height transitions;
    /// forms retain their top edge. Bodies scroll at the available-height limit.
    fn surface(
        &self,
        dialog: Dialog,
        id: u64,
        width: f32,
        edge: f32,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Dialog {
        let p = self.palette;
        let measured = {
            let parts = self.ui.dialog_parts.borrow();
            parts
                .get(&(id, 0))
                .copied()
                .zip(parts.get(&(id, 1)).copied())
        };
        // Until the first measurement the dialog stays invisible, so it never
        // shows at the default position and then jumps.
        // The component moves each stacked dialog 16 px lower (undone here) and
        // keeps 16 px from the window's edges; stacked confirmations are
        // centered in the same way as other dialogs.
        let layer = self
            .ui
            .below
            .iter()
            .position(|m| m.id == id)
            .unwrap_or(self.ui.below.len());
        let shift = px(16. * layer as f32);
        let dialog = match measured {
            Some((body, footer)) => {
                let viewport = window.viewport_size().height;
                let edge = px(edge);
                // Body, footer and the 1 px border above and below.
                let natural = body + footer + px(2.);
                // The tallest the component draws a dialog at this layer.
                let limit = (viewport - px(32.) - shift).max(px(0.));
                let now = Instant::now();
                let length = if cx.reduce_motion() {
                    Duration::ZERO
                } else {
                    theme::motion_duration()
                };
                let mut frames = self.ui.dialog_frames.borrow_mut();
                let frame = frames.entry(id).or_insert(DialogFrame {
                    top: edge,
                    viewport: px(-1.),
                    from: natural,
                    to: natural,
                    since: now,
                    clip: None,
                    body_height: px(0.),
                    scroll: ScrollHandle::new(),
                });
                let centered = self
                    .find_modal(id)
                    .is_some_and(|modal| matches!(modal.kind.as_str(), "update" | "whatsnew"));
                let (height, cap) = frame.place(
                    natural,
                    DialogLayout {
                        viewport,
                        edge,
                        limit,
                        centered,
                    },
                    now,
                    length,
                );
                let target = frame.to;
                let easing = height != frame.to;
                if easing {
                    window.request_animation_frame();
                }
                // 1 px short of the body's room, so that rounding to device
                // pixels never leaves a fraction of a pixel to scroll.
                frame.clip =
                    (easing || natural <= cap).then(|| (height - footer - px(3.)).max(px(0.)));
                frame.body_height = (height - footer - px(2.)).max(px(0.));
                if let Some(ms) = scrollbar::trace_ms() {
                    println!(
                        "{}",
                        json!({"event":"dialog","ms":ms,"id":id,"top":f32::from(frame.top),
                        "height":f32::from(height),"target":f32::from(target),
                        "natural":f32::from(natural),"clip":frame.clip.map(f32::from)})
                    );
                }
                let top = frame.top;
                drop(frames);
                dialog
                    .margin_top(top - shift)
                    .h(height)
                    .opacity(self.dialog_opacity(id, window, cx))
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
            // The interface's 13 px body text; the component's default is larger.
            .text_size(theme::font(13.))
            .bg(widgets::solid(p.flyout))
            .border_color(p.stroke_strong)
            .rounded(px(8.))
            .shadow(widgets::flyout_shadow(p.dark))
            .close_button(false)
            // The entrance, the exit and the scrim (`Scrim`, drawn over the chart
            // layer too) are drawn separately; the backdrop only takes the clicks.
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
                                // Escape counts as done, as the interface's close().
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
    /// (`dialog-out`, accelerating). The interface's slight scale is left out.
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
            .font_weight(theme::LABEL_WEIGHT)
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

    pub(in crate::frontend) fn primary_focus(&self, id: u64, cx: &App) -> FocusHandle {
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
        // How far each dialog stays from the window's top and bottom edges
        // (the interface's `max-height`/`height` margins; 16 px by default).
        match modal.kind.as_str() {
            "onboarding" => {
                // Only its own buttons and Escape close the onboarding.
                let dialog = self
                    .surface(dialog, id, 580., 16., window, cx)
                    .overlay_closable(false);
                self.onboarding_dialog(dialog, &modal, window, cx)
            }
            "shortcuts" => {
                let dialog = self.surface(dialog, id, 780., 24., window, cx);
                self.shortcuts_dialog(dialog, id, cx)
            }
            "columns" => {
                let dialog = self
                    .surface(dialog, id, 900., 16., window, cx)
                    .overlay_closable(false);
                self.process_columns_dialog(dialog, &modal, window, cx)
            }
            "license" | "notices" => {
                let dialog = self.surface(dialog, id, 720., 16., window, cx);
                self.document_dialog(dialog, &modal, cx)
            }
            "task" => {
                let dialog = self
                    .surface(dialog, id, 540., 20., window, cx)
                    .bg(widgets::solid(theme::token("window-bg", self.palette.dark)));
                self.task_dialog(dialog, &modal, window, cx)
            }
            "filelock" => {
                let dialog = self.surface(dialog, id, 620., 16., window, cx);
                self.filelock_dialog(dialog, &modal, window, cx)
            }
            "update" | "whatsnew" => {
                let dialog = self.surface(dialog, id, 620., 32., window, cx);
                self.update_dialog(dialog, &modal, window, cx)
            }
            "note" => {
                let dialog = self.surface(dialog, id, 420., 16., window, cx);
                self.note_dialog(dialog, &modal, window, cx)
            }
            "visibility" => {
                let dialog = self.surface(dialog, id, 580., 16., window, cx);
                self.visibility_dialog(dialog, &modal, window, cx)
            }
            _ => {
                let dialog = self.surface(dialog, id, 420., 16., window, cx);
                self.confirm_view(dialog, &modal, cx)
            }
        }
    }

    /// A confirmation: title and text, then
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
                self.dialog_body(id).child(
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
            "columns" if !in_input => self.apply_process_columns(window, cx),
            "columns" => {}
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
            "visibility" => {
                self.save_visibility(window, cx);
                return;
            }
            "confirm" => {
                if let Some(ask) = modal.ask {
                    self.close_overlay(window, cx);
                    (ask.then)(self, window, cx);
                }
                return;
            }
            "columns" => {
                self.apply_process_columns(window, cx);
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

    fn note_dialog(
        &self,
        dialog: Dialog,
        modal: &Modal,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Dialog {
        let p = self.palette;
        let id = modal.id;
        dialog
            .child(
                self.dialog_body(id).child(
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
                            d.child(self.text_field(
                                input,
                                widgets::input(input),
                                Field::Boxed,
                                label("common.marks.placeholder").into(),
                                true,
                                window,
                                cx,
                            ))
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

    /// A long text in a bordered monospaced box.
    fn document_dialog(&self, dialog: Dialog, modal: &Modal, cx: &mut Context<Self>) -> Dialog {
        let p = self.palette;
        let id = modal.id;
        let document = scrollbar::fresh_handle("document", id);
        dialog
            .child(
                self.dialog_body(id).child(
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

    /// A determinate (`value`) or sweeping progress bar.
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
        let image = id.and_then(|id| self.process_image(id));
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

    // ---- Toasts -------------------

    /// Apply tray navigation before the independent background-cadence notice.
    pub(in crate::frontend) fn restored(
        &mut self,
        value: &Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A restore and hide between two visibility polls starts a new hidden period.
        self.hidden_since = None;
        // Leave unfinished dialog work in its original page context. Reopening
        // the current default also keeps its selection, scroll and filter.
        if self.modal.is_none()
            && let Some(page) = tray_page(
                &self.prefs.get::<String>("trayPage", "current".into()),
                &self.prefs.get("startPage", Value::Null),
                value,
            )
            && self.page != page
        {
            self.ui.select = None;
            self.ui.select_leaving = None;
            self.act(Action::Page(page.into()), window, cx);
        }
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
                &[
                    (
                        "from",
                        &crate::regional::numeric((shown / 1000.).to_string()),
                    ),
                    (
                        "to",
                        &crate::regional::numeric((hidden / 1000.).to_string()),
                    ),
                ],
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
    ///.
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
            if self.ui.system_motion.is_some_and(|before| before != system) {
                self.apply_motion(cx);
                changed = true;
            }
            self.ui.system_motion = Some(system);
        }
        if self.ui.ticks.is_multiple_of(4) {
            self.announce_update(window, cx);
            changed |= self.announce_star();
        }
        changed |= self.tick_star();
        changed |= self.announce_administrator();
        changed |= self.tick_hotkey(window, cx);
        changed |= self.tick_settings();
        if changed {
            window.refresh();
            cx.notify();
        }
    }

    fn toast_layer(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        // The star request keeps the corner; notices stack above it.
        let (star, star_moving) = self.star_card(window, cx).unzip();
        if self.ui.toasts.is_empty() && star.is_none() {
            return None;
        }
        let p = self.palette;
        let reduce = cx.reduce_motion();
        let viewport = window.viewport_size();
        let width = px(380.).min(viewport.width - px(40.));
        let mut animating = star_moving.unwrap_or(false);
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
                    // No shadow, as the interface's toasts (`.toast`).
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
                                                .when(self.options.test, |button| {
                                                    let anchors = self.ui.anchors.clone();
                                                    button.child(
                                                        canvas(
                                                            move |bounds, _, _| {
                                                                anchors.borrow_mut().insert(
                                                                    format!(
                                                                        "toast-action-{id}-{index}"
                                                                    )
                                                                    .into(),
                                                                    bounds,
                                                                );
                                                            },
                                                            |_, _, _, _| {},
                                                        )
                                                        .absolute()
                                                        .inset_0(),
                                                    )
                                                })
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
            }))
            .children(star);
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

/// Keeps a dialog part's measured height (`Desktop::surface` sizes the dialog
/// from it); a change draws the window again.
fn note_part(
    parts: &RefCell<HashMap<(u64, u8), Pixels>>,
    key: (u64, u8),
    top: Pixels,
    height: Pixels,
    window: &mut Window,
    cx: &mut App,
) {
    if let Some(ms) = scrollbar::trace_ms() {
        println!(
            "{}",
            json!({"event":"dialogPart","ms":ms,"id":key.0,"slot":key.1,
            "y":f32::from(top),"h":f32::from(height)})
        );
    }
    if parts
        .borrow()
        .get(&key)
        .is_none_or(|previous| part_height_changed(*previous, height, window.scale_factor()))
    {
        parts.borrow_mut().insert(key, height);
        window.defer(cx, |window, _| window.refresh());
    }
}

fn part_height_changed(previous: Pixels, measured: Pixels, scale: f32) -> bool {
    // Subtracting absolute bounds at fractional scale can alternate by an ULP.
    // Keep the last stable measurement instead of restarting the height animation.
    (f32::from(measured - previous) * scale).abs() >= 1. / 64.
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
        // The caption buttons stay above the dim, as the interface's (`.caption`
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
