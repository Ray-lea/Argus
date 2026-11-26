//! Language and appearance: the language choice, the process overview band,
//! and the interface scale and font size boxes with their spin buttons.
use super::*;
use crate::frontend::widgets::{ButtonExt, Tone};

impl Desktop {
    /// The language choice (also used by the onboarding).
    pub(in crate::frontend) fn language_select(
        &self,
        id: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let values = std::iter::once("system")
            .chain(
                argus_core::i18n::Locale::ALL
                    .into_iter()
                    .map(|locale| locale.tag()),
            )
            .collect::<Vec<_>>();
        let current = text(&self.data["settings"], "language");
        self.select(
            id.to_owned(),
            label("settings.language.title"),
            values
                .iter()
                .map(|tag| {
                    if *tag == "system" {
                        label("common.languageSystem")
                    } else {
                        label(&format!("common.languageName.{}", tag.replace('-', "")))
                    }
                })
                .collect(),
            values.iter().position(|v| *v == current),
            false,
            move |this, index, window, cx| {
                this.request(
                    "set_language",
                    json!({"language":values[index]}),
                    Some("settings"),
                    window,
                    cx,
                );
            },
            cx,
        )
    }

    pub(super) fn vitals_mode_setting(&self, cx: &mut Context<Self>) -> AnyElement {
        let modes = ["auto", "show", "hide"];
        let current = self.prefs.get::<String>("vitalsMode", "auto".into());
        self.select(
            "settings-vitals-mode",
            label("settings.vitals.title"),
            modes
                .iter()
                .map(|mode| label(&format!("settings.vitals.{mode}")))
                .collect(),
            Some(modes.iter().position(|mode| *mode == current).unwrap_or(0)),
            false,
            move |this, index, window, cx| {
                this.preference("vitalsMode", json!(modes[index]), window, cx)
            },
            cx,
        )
    }

    // ---- Appearance ---------------------------------------------------------------

    /// Interface scale or font size: a short number box with its
    /// unit (`.appearance-number`) and a button back to the default. The box
    /// commits on Enter, when it loses focus and when its spin button is let
    /// go; the arrow keys step it and commit.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn appearance_setting(
        &mut self,
        setting: &'static appearance::Setting,
        id: &str,
        glyph: &str,
        title: &str,
        desc: &str,
        unit: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.palette;
        let value = self.appearance_value(setting);
        let input = self.appearance_input(setting, window, cx);
        let focused = input.read(cx).focus_handle(cx).is_focused(window);
        let held = self
            .ui
            .spin
            .as_ref()
            .is_some_and(|spin| std::ptr::eq(spin.setting, setting));
        let group = SharedString::from(format!("setting-{id}-number"));
        let number = div()
            .flex()
            .items_center()
            .gap(px(6.))
            .child(
                // Three digits scale with the font. The spin button, frame
                // padding and borders, and the editor's caret margin are fixed.
                div()
                    .id(SharedString::from(format!("setting-{id}-input")))
                    .group(group.clone())
                    .relative()
                    .w(theme::font(13. * 2.25) + px(15. + 16. + 10.))
                    .h(theme::font(13.) * 1.4 + px(12.))
                    .px(px(7.))
                    .flex()
                    .items_center()
                    .border_1()
                    .border_color(p.stroke)
                    .rounded(px(4.))
                    .bg(p.control_face)
                    .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                        let up = match event.keystroke.key.as_str() {
                            "up" => true,
                            "down" => false,
                            _ => return,
                        };
                        cx.stop_propagation();
                        let Some(input) = this.ui.inputs.get(setting.key).cloned() else {
                            return;
                        };
                        if let Some(next) = setting.spin(&input.read(cx).value(), up) {
                            this.commit_appearance(setting, next, window, cx);
                        }
                    }))
                    .child(
                        // The input keeps its own 32 px height: centred, its
                        // line sits where the box's text did.
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .flex()
                            .items_center()
                            .child(
                                widgets::input(&input)
                                    .appearance(false)
                                    .px_0()
                                    .py_0()
                                    .text_size(theme::font(13.))
                                    // Named, for assistive technology, by
                                    // the setting it changes.
                                    .aria_label(label(title)),
                            ),
                    )
                    .child(self.spin_button(setting, id, &group, focused || held, window, cx))
                    // `:focus-visible`: the 2 px ring in the text colour.
                    .when(focused, |d| d.child(widgets::focus_ring(p.ink, px(1.)))),
            )
            .child(div().text_color(p.ink).child(unit))
            .into_any_element();
        let default = setting.default;
        let reset = self
            .btn(
                format!("setting-{id}-reset"),
                label("settings.appearance.reset"),
                None,
                Tone::Plain,
            )
            .inert(value == default)
            .on_click(cx.listener(move |this, _, window, cx| {
                this.commit_appearance(setting, default, window, cx);
            }))
            .into_any_element();
        self.setting_text(id, glyph, title, desc, vec![number, reset])
    }

    fn appearance_value(&self, setting: &appearance::Setting) -> u32 {
        if setting.key == appearance::SCALE.key {
            self.ui_scale
        } else {
            self.font_size
        }
    }

    /// The number box of an appearance setting, kept across frames.
    fn appearance_input(
        &mut self,
        setting: &'static appearance::Setting,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        if let Some(input) = self.ui.inputs.get(setting.key).cloned() {
            return input;
        }
        let value = self.appearance_value(setting).to_string();
        let input = cx.new(|cx| {
            let mut state = InputState::new(window, cx);
            state.set_value(value, window, cx);
            state
        });
        self.subscriptions.push(cx.subscribe_in(
            &input,
            window,
            move |this, input, event, window, cx| match event {
                // Keep composition and pasted separators intact; validate on commit.
                InputEvent::PressEnter { .. } | InputEvent::Blur => {
                    let next = setting.parse(&input.read(cx).value());
                    this.commit_appearance(setting, next, window, cx);
                }
                _ => {}
            },
        ));
        self.ui.inputs.insert(setting.key.into(), input.clone());
        input
    }

    /// Applies and saves an appearance value, and shows it in its box as
    /// committed (text that is not a number shows the default again).
    fn commit_appearance(
        &mut self,
        setting: &'static appearance::Setting,
        value: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if value != self.appearance_value(setting) {
            self.preference(setting.key, json!(value), window, cx);
        }
        if let Some(input) = self.ui.inputs.get(setting.key).cloned() {
            let text = value.to_string();
            if input.read(cx).value().as_ref() != text {
                input.update(cx, |state, cx| state.set_value(text, window, cx));
            }
        }
    }

    /// A number box's spin button (`::-webkit-inner-spin-button`, drawn by
    /// Chromium): 15 px wide at the end of the box's text, its two halves
    /// stepping up and down. It shows while the box is pointed at or focused;
    /// the arrow under the pointer lights up. Held, it steps once, again after
    /// 250 ms and then every 50 ms the way the half under the pointer points,
    /// as Chromium repeats it; the box commits when it is let go.
    fn spin_button(
        &self,
        setting: &'static appearance::Setting,
        id: &str,
        group: &SharedString,
        shown: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let dark = self.palette.dark;
        let scale = window.scale_factor();
        let half = |up: bool| {
            let lit = self.ui.spin_hover == Some((setting.key, up));
            let color = theme::token(
                if lit {
                    "spin-arrow-hover"
                } else {
                    "spin-arrow"
                },
                dark,
            );
            let arrow = assets::spin_arrow(up, color, scale, cx);
            div()
                .id(SharedString::from(format!(
                    "setting-{id}-{}",
                    if up { "up" } else { "down" }
                )))
                .relative()
                .flex_1()
                .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                    let here = Some((setting.key, up));
                    if *hovered {
                        this.ui.spin_hover = here;
                    } else if this.ui.spin_hover == here {
                        this.ui.spin_hover = None;
                    }
                    cx.notify();
                }))
                .child(
                    // 15 × 9 in the middle of the half, on whole device pixels,
                    // area-filtered from its double-resolution raster.
                    canvas(
                        |_, _, _| {},
                        move |bounds, _, window, _| {
                            let Some(arrow) = arrow else {
                                return;
                            };
                            let s = window.scale_factor();
                            let snap = |v: Pixels| px((f32::from(v) * s).round() / s);
                            let size = size(px(15.), px(9.));
                            let origin = bounds.center() - point(size.width / 2., size.height / 2.);
                            let _ = window.paint_image_subpixel(
                                Bounds::new(point(snap(origin.x), snap(origin.y)), size),
                                arrow,
                                0,
                            );
                        },
                    )
                    .absolute()
                    .size_full(),
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.spin_press(setting, up, window, cx);
                    }),
                )
        };
        // Wherever the button is let go, and if the press got lost (the
        // pointer moving with no button down), the hold ends.
        let owner = cx.entity().downgrade();
        let track = canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                let held = move |owner: &WeakEntity<Desktop>, cx: &App| {
                    owner.upgrade().filter(|desktop| {
                        desktop
                            .read(cx)
                            .ui
                            .spin
                            .as_ref()
                            .is_some_and(|spin| std::ptr::eq(spin.setting, setting))
                    })
                };
                let moved = owner.clone();
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                    if phase != DispatchPhase::Capture {
                        return;
                    }
                    let Some(desktop) = held(&moved, cx) else {
                        return;
                    };
                    let over = bounds
                        .contains(&event.position)
                        .then(|| event.position.y < bounds.center().y);
                    let pressed = event.pressed_button == Some(MouseButton::Left);
                    desktop.update(cx, |this, cx| this.spin_track(over, pressed, window, cx));
                });
                window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
                    if phase != DispatchPhase::Capture || event.button != MouseButton::Left {
                        return;
                    }
                    if let Some(desktop) = held(&owner, cx) {
                        desktop.update(cx, |this, cx| this.spin_release(window, cx));
                    }
                });
            },
        )
        .absolute()
        .size_full();
        div()
            .relative()
            .flex_shrink_0()
            .w(px(15.))
            .h(theme::font(13.) * 1.4)
            .flex()
            .flex_col()
            .bg(theme::token("spin-face", dark))
            .opacity(if shown { 1. } else { 0. })
            .when(!shown, |d| d.group_hover(group.clone(), |s| s.opacity(1.)))
            .child(half(true))
            .child(half(false))
            .child(track)
    }

    /// The spin button pressed: the box takes focus and steps, and keeps
    /// stepping while it is held.
    fn spin_press(
        &mut self,
        setting: &'static appearance::Setting,
        up: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(input) = self.ui.inputs.get(setting.key).cloned() else {
            return;
        };
        input.update(cx, |state, cx| state.focus(window, cx));
        self.spin_step(setting, up, window, cx);
        let repeat = cx.spawn_in(window, async move |this, cx| {
            // Steps keep to a schedule from the press: a timer fires on the system
            // tick (15.6 ms by default), so waiting the interval after each step
            // would repeat every 62.5 ms instead of every 50 on average.
            let pressed = cx.background_executor().now();
            for step in 0u32.. {
                let due = pressed + scrollbar::REPEAT_DELAY + scrollbar::REPEAT_INTERVAL * step;
                let now = cx.background_executor().now();
                cx.background_executor()
                    .timer(due.saturating_duration_since(now))
                    .await;
                let held = this.update_in(cx, |this, window, cx| {
                    let Some(spin) = this.ui.spin.as_ref() else {
                        return false;
                    };
                    if let Some(up) = spin.over {
                        this.spin_step(setting, up, window, cx);
                    }
                    true
                });
                if !matches!(held, Ok(true)) {
                    break;
                }
            }
        });
        self.ui.spin = Some(widgets::Spin {
            setting,
            over: Some(up),
            repeat,
        });
        cx.notify();
    }

    /// One step of the box's text; it commits when the button is let go.
    fn spin_step(
        &mut self,
        setting: &'static appearance::Setting,
        up: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(input) = self.ui.inputs.get(setting.key).cloned() else {
            return;
        };
        let text = input.read(cx).value();
        if let Some(next) = setting.spin(&text, up).map(|next| next.to_string())
            && next != text.as_ref()
        {
            input.update(cx, |state, cx| state.set_value(next, window, cx));
        }
    }

    /// The pointer moved while the spin button is held.
    fn spin_track(
        &mut self,
        over: Option<bool>,
        pressed: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !pressed {
            self.spin_release(window, cx);
        } else if let Some(spin) = &mut self.ui.spin {
            spin.over = over;
        }
    }

    /// Releasing the spin button commits the value shown in the box.
    fn spin_release(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(spin) = self.ui.spin.take() else {
            return;
        };
        drop(spin.repeat);
        if let Some(input) = self.ui.inputs.get(spin.setting.key).cloned() {
            let next = spin.setting.parse(&input.read(cx).value());
            self.commit_appearance(spin.setting, next, window, cx);
        }
        cx.notify();
    }
}
