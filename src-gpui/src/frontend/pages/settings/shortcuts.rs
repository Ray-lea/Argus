//! The keyboard shortcuts sheet, opened from the hotkey section: the keys
//! and pointer gestures of the pages, tables and history.
use super::*;
use crate::frontend::recorder;
use crate::frontend::widgets::Tone;

impl Desktop {
    // ---- Keyboard shortcuts sheet ------------------------

    /// Keys as keycaps: chords split by " / ", keys by "+".
    /// A pointer gesture in words, in a quieter frame than a key's.
    fn shortcut_gesture(&self, gesture: &str) -> AnyElement {
        let p = self.palette;
        div()
            .flex_shrink_0()
            .h(px(25.))
            .px(px(8.))
            .flex()
            .items_center()
            .border_1()
            .border_color(p.stroke)
            .rounded(px(5.))
            .text_size(theme::font(11.))
            .text_color(p.ink)
            .whitespace_nowrap()
            .child(gesture.to_owned())
            .into_any_element()
    }

    pub(in crate::frontend) fn shortcut_keys(&self, value: &str) -> AnyElement {
        let p = self.palette;
        let mut row = div().flex().flex_wrap().items_center().gap(px(6.));
        for (index, chord) in value.split(" / ").enumerate() {
            if index > 0 {
                row = row.child(
                    div()
                        .text_size(theme::font(10.))
                        .text_color(p.faint)
                        .child("/"),
                );
            }
            let mut keys = div().flex().items_center().gap(px(4.)).whitespace_nowrap();
            // The plus key alone is a key, not a chord of two empty ones.
            let parts = if chord == "+" {
                vec![chord]
            } else {
                chord.split('+').collect::<Vec<_>>()
            };
            for (part, key) in parts.into_iter().enumerate() {
                if part > 0 {
                    keys = keys.child(
                        div()
                            .text_size(theme::font(10.))
                            .text_color(p.faint)
                            .child("+"),
                    );
                }
                keys = keys.child(
                    div()
                        .min_w(px(24.))
                        .h(px(25.))
                        .px(px(6.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .border_1()
                        .border_b_2()
                        .border_color(p.stroke_strong)
                        .rounded(px(5.))
                        .bg(p.face)
                        .text_size(theme::font(11.))
                        .text_color(p.ink)
                        .child(recorder::key_label(key)),
                );
            }
            row = row.child(keys);
        }
        row.into_any_element()
    }

    pub(in crate::frontend) fn shortcuts_dialog(
        &self,
        dialog: gpui_kit::component::dialog::Dialog,
        id: u64,
        cx: &mut Context<Self>,
    ) -> gpui_kit::component::dialog::Dialog {
        let p = self.palette;
        let settings = &self.data["settings"];
        let hotkey = text(settings, "hotkey").to_owned();
        let global = boolean(settings, "hotkeyEnabled")
            && !hotkey.is_empty()
            && settings["hotkeyError"].is_null();
        let open = self
            .modal
            .as_ref()
            .filter(|m| m.id == id)
            .map(|m| m.args["open"].clone())
            .unwrap_or_default();
        let card =
            |glyph: &str, title: String, keys: AnyElement, body: String, badge: Option<String>| {
                div()
                    .flex_1()
                    .min_w_0()
                    .px(px(16.))
                    .py(px(14.))
                    .border_1()
                    .border_color(p.stroke)
                    .rounded(px(8.))
                    .bg(p.layer)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .mb(px(10.))
                            .font_weight(theme::LABEL_WEIGHT)
                            .child(icon(glyph, p.accent))
                            .child(title)
                            .when_some(badge, |d, badge| {
                                d.child(
                                    div()
                                        .ml_auto()
                                        .text_size(theme::font(10.))
                                        .font_weight(FontWeight::NORMAL)
                                        .text_color(p.faint)
                                        .child(badge),
                                )
                            }),
                    )
                    .child(keys)
                    .child(
                        div()
                            .mt(px(9.))
                            .text_size(theme::font(12.))
                            .line_height(relative(1.6))
                            .text_color(p.muted)
                            .child(body),
                    )
            };
        let common = [
            (
                "PauseIcon",
                "settings.shortcuts.pauseTitle",
                "settings.shortcuts.pause",
                "Ctrl",
            ),
            (
                "EndIcon",
                "common.endProcess",
                "settings.shortcuts.end",
                "Alt+E",
            ),
            (
                "SearchIcon",
                "settings.shortcuts.searchTitle",
                "settings.shortcuts.search",
                "Ctrl+F",
            ),
            (
                "RunTaskIcon",
                "tasks.title",
                "settings.shortcuts.runTask",
                "Ctrl+N",
            ),
        ];
        let mut cards = common
            .into_iter()
            .map(|(glyph, title, body, keys)| {
                card(
                    glyph,
                    label(title),
                    self.shortcut_keys(keys),
                    label(body),
                    None,
                )
            })
            .collect::<Vec<_>>();
        cards.push(card(
            "KeyboardIcon",
            label("settings.hotkey.title"),
            if hotkey.is_empty() {
                div()
                    .text_size(theme::font(12.))
                    .text_color(p.faint)
                    .child(label("settings.shortcuts.disabled"))
                    .into_any_element()
            } else {
                self.shortcut_keys(&recorder::shortcut_keys(&hotkey).join("+"))
            },
            label(if global {
                "settings.shortcuts.globalHint"
            } else {
                "settings.shortcuts.disabled"
            }),
            Some(label("settings.shortcuts.global")),
        ));
        let mut rows = div().flex().flex_col().gap(px(10.));
        let mut iter = cards.into_iter();
        while let Some(first) = iter.next() {
            let mut row = div().flex().gap(px(10.)).child(first);
            if let Some(second) = iter.next() {
                row = row.child(second);
            }
            rows = rows.child(row);
        }
        let pages = div()
            .flex()
            .flex_wrap()
            .gap_y(px(8.))
            .children(PAGES.iter().enumerate().map(|(index, (page, glyph))| {
                div()
                    .w(relative(0.25))
                    .pr(px(14.))
                    .flex()
                    .items_center()
                    .gap(px(7.))
                    .min_w_0()
                    .child(icon(glyph, p.muted).size(px(15.)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(label(&format!("shell.page.{page}"))),
                    )
                    .child(self.shortcut_keys(&format!("Ctrl+{}", index + 1)))
            }));
        type Group<'a> = (&'a str, &'a str, &'a str, &'a [(&'a str, &'a str)]);
        let groups: [Group; 3] = [
            (
                "table",
                "ProcessesIcon",
                "settings.shortcuts.table",
                &[
                    ("↑ / ↓", "settings.shortcuts.select"),
                    ("PageUp / PageDown", "settings.shortcuts.page"),
                    ("Home / End", "settings.shortcuts.ends"),
                    ("← / →", "settings.shortcuts.expand"),
                    ("Enter", "settings.shortcuts.details"),
                    ("Ctrl+C", "settings.shortcuts.copyName"),
                    ("Ctrl+Shift+C", "settings.shortcuts.copyDetails"),
                    ("Delete", "settings.shortcuts.endTable"),
                    ("Shift+Delete", "settings.shortcuts.endDirect"),
                    ("Esc", "settings.shortcuts.clear"),
                    // A gesture, named in words.
                    (
                        "settings.shortcuts.mouse.shiftWheel",
                        "settings.shortcuts.scrollSideways",
                    ),
                ],
            ),
            (
                "history",
                "HistoryIcon",
                "settings.shortcuts.history",
                &[
                    ("← / →", "settings.shortcuts.time"),
                    ("Shift+← / Shift+→", "settings.shortcuts.timeJump"),
                    ("PageUp / PageDown", "settings.shortcuts.timeJump"),
                    ("Home", "settings.shortcuts.timeFirst"),
                    ("End", "settings.shortcuts.latest"),
                    ("+ / −", "settings.shortcuts.zoom"),
                    ("0", "settings.shortcuts.zoomReset"),
                    ("Esc", "settings.shortcuts.clearCursor"),
                ],
            ),
            // Gestures, named in words rather than drawn as keys.
            (
                "historyMouse",
                "PointerIcon",
                "settings.shortcuts.historyMouse",
                &[
                    (
                        "settings.shortcuts.mouse.hover",
                        "settings.shortcuts.mouse.hoverText",
                    ),
                    (
                        "settings.shortcuts.mouse.click",
                        "settings.shortcuts.mouse.clickText",
                    ),
                    (
                        "settings.shortcuts.mouse.drag",
                        "settings.shortcuts.mouse.dragText",
                    ),
                    (
                        "settings.shortcuts.mouse.right",
                        "settings.shortcuts.mouse.rightText",
                    ),
                    (
                        "settings.shortcuts.mouse.pin",
                        "settings.shortcuts.mouse.pinText",
                    ),
                    (
                        "settings.shortcuts.mouse.wheel",
                        "settings.shortcuts.mouse.wheelText",
                    ),
                    (
                        "settings.shortcuts.mouse.shiftWheel",
                        "settings.shortcuts.mouse.shiftWheelText",
                    ),
                    (
                        "settings.shortcuts.mouse.double",
                        "settings.shortcuts.mouse.doubleText",
                    ),
                    (
                        "settings.shortcuts.mouse.navigator",
                        "settings.shortcuts.mouse.navigatorText",
                    ),
                    (
                        "settings.shortcuts.mouse.divider",
                        "settings.shortcuts.mouse.dividerText",
                    ),
                    (
                        "settings.shortcuts.mouse.grip",
                        "settings.shortcuts.mouse.gripText",
                    ),
                    (
                        "settings.shortcuts.mouse.reading",
                        "settings.shortcuts.mouse.readingText",
                    ),
                ],
            ),
        ];
        let mut body = div()
            .flex()
            .flex_col()
            .pt(px(20.))
            .px(px(24.))
            .pb(px(20.))
            .child(rows)
            .child(
                div()
                    .flex()
                    .justify_between()
                    .items_center()
                    .mt(px(20.))
                    .mb(px(10.))
                    .child(
                        div()
                            .font_weight(theme::LABEL_WEIGHT)
                            .child(label("settings.shortcuts.navigation")),
                    )
                    .child(self.shortcut_keys(&format!("Ctrl+1–{}", PAGES.len()))),
            )
            .child(div().mb(px(20.)).child(pages));
        for (key, glyph, title, items) in groups {
            let expanded = boolean(&open, key);
            body = body.child(
                div()
                    .mb(px(8.))
                    .border_1()
                    .border_color(p.stroke)
                    .rounded(px(8.))
                    .bg(p.layer)
                    .child(
                        gpui_kit::base::Button::new(SharedString::from(format!(
                            "shortcut-group-{key}"
                        )))
                        .w_full()
                        .line_height(theme::line_height())
                        .h(px(40.))
                        .px(px(14.))
                        .gap(px(8.))
                        .justify_start()
                        .rounded(px(8.))
                        .hover(move |s| s.bg(p.hover))
                        .child(icon(glyph, p.accent))
                        .child(
                            div()
                                .flex_1()
                                .font_weight(theme::LABEL_WEIGHT)
                                .child(label(title)),
                        )
                        .child(
                            icon(if expanded { "ChevronUp" } else { "ChevronDown" }, p.muted)
                                .size(px(14.)),
                        )
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                if let Some(modal) = &mut this.modal
                                    && modal.id == id
                                {
                                    let now = !boolean(&modal.args["open"], key);
                                    if !modal.args["open"].is_object() {
                                        modal.args["open"] = json!({});
                                    }
                                    modal.args["open"][key] = json!(now);
                                }
                                window.refresh();
                                cx.notify();
                            },
                        )),
                    )
                    .when(expanded, |d| {
                        d.child(
                            div()
                                .px(px(14.))
                                .pb(px(12.))
                                .flex()
                                .flex_col()
                                .gap(px(8.))
                                .children(items.iter().map(|(keys, text_key)| {
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .gap(px(16.))
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w_0()
                                                .text_size(theme::font(12.))
                                                .text_color(p.muted)
                                                .child(label(text_key)),
                                        )
                                        .child(if keys.starts_with("settings.") {
                                            self.shortcut_gesture(&label(keys))
                                        } else {
                                            self.shortcut_keys(keys)
                                        })
                                })),
                        )
                    }),
            );
        }
        body = body
            .child(
                div()
                    .mt(px(12.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .text_size(theme::font(12.))
                    .text_color(p.muted)
                    .child(self.shortcut_keys("Ctrl+Shift+Esc"))
                    .child(label("settings.shortcuts.taskManager")),
            )
            .child(
                div()
                    .mt(px(12.))
                    .text_size(theme::font(12.))
                    .text_color(p.faint)
                    .child(label("settings.shortcuts.inputHint")),
            );
        let header = div()
            .flex()
            .items_center()
            .gap(px(14.))
            .pt(px(22.))
            .px(px(24.))
            .pb(px(18.))
            .border_b_1()
            .border_color(p.stroke)
            .child(
                div()
                    .size(px(44.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .border_1()
                    .border_color(p.accent.opacity(0.24))
                    .rounded(px(12.))
                    .bg(p.accent.opacity(0.1))
                    .child(icon("KeyboardIcon", p.accent).size(px(24.))),
            )
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(self.dialog_title(label("settings.shortcuts.title")))
                    .child(
                        div()
                            .text_size(theme::font(12.))
                            .text_color(p.muted)
                            .child(label("settings.shortcuts.scope")),
                    ),
            )
            .child(
                self.btn(
                    format!("modal-{id}-x"),
                    "",
                    Some("DismissIcon"),
                    Tone::Plain,
                )
                .accessibility_label(label("common.close"))
                .on_click(cx.listener(|this, _, window, cx| this.close_overlay(window, cx))),
            );
        // As tall as its content: `surface` grows it downward, keeping its top.
        dialog
            .child(
                self.dialog_body(id)
                    .child(div().flex().flex_col().child(header).child(body)),
            )
            .footer(
                self.measure(id, 1).child(
                    self.dialog_footer()
                        // `.shortcut-dialog .dialog-footer`: 12 px above and below.
                        .py(px(12.))
                        .justify_between()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .text_size(theme::font(12.))
                                .text_color(p.muted)
                                .child(self.shortcut_keys("Esc"))
                                .child(label("settings.shortcuts.close")),
                        )
                        .child(
                            self.btn(
                                format!("modal-{id}-close"),
                                label("common.close"),
                                None,
                                Tone::Accent,
                            )
                            .min_w(px(96.))
                            .track_focus(&self.focus_for(&format!("modal-{id}-primary"), cx))
                            .on_click(
                                cx.listener(|this, _, window, cx| this.close_overlay(window, cx)),
                            ),
                        ),
                ),
            )
    }
}
