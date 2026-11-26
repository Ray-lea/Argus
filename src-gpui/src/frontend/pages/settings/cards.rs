//! The frames of the settings cards (a setting, a feature with its state,
//! the outline of a card pointed out from elsewhere) and the controls they
//! share: toggles that run a command, segmented choices, inline text boxes.
//! The About and Components pages and the history's settings use them too.
use super::*;

impl Desktop {
    /// A card glyph at the cards' 20 px.
    pub(in crate::frontend) fn glyph(&self, name: &str) -> AnyElement {
        icon(name, self.palette.ink)
            .size(px(20.))
            .into_any_element()
    }

    /// The outline a card pointed out from elsewhere wears for a moment.
    pub(super) fn located_tint(&self, id: &str) -> f32 {
        self.ui
            .located
            .as_ref()
            .filter(|(target, _)| target == id)
            .map(|(_, at)| 1. - theme::ease_out((at.elapsed().as_secs_f32() / 1.8).min(1.)))
            .unwrap_or(0.)
    }

    /// A located card's border and fill: eased
    /// from the accent and from 12 % accent back to the card's own.
    pub(super) fn located_colors(&self, tint: f32) -> (Hsla, Hsla) {
        let p = self.palette;
        if tint <= 0. {
            return (p.stroke, p.layer);
        }
        (
            theme::mix(p.stroke, p.accent, tint),
            theme::mix(p.layer, theme::mix(p.layer, p.accent, 0.12), tint),
        )
    }

    /// A located card's 1 px accent ring just outside its border (the
    /// keyframe's `box-shadow: 0 0 0 1px`), fading with it. Absolute inside
    /// the card: a GPUI shadow would tint the translucent card under it.
    pub(super) fn located_ring(&self, tint: f32) -> Option<AnyElement> {
        (tint > 0.).then(|| {
            div()
                .absolute()
                .top(px(-2.))
                .left(px(-2.))
                .right(px(-2.))
                .bottom(px(-2.))
                .rounded(px(7.))
                .border_1()
                .border_color(self.palette.accent.opacity(tint))
                .into_any_element()
        })
    }

    /// One setting (`.setting`): icon, title and description, controls on the right.
    pub(in crate::frontend) fn setting(
        &self,
        id: &str,
        glyph: AnyElement,
        title: impl IntoElement,
        desc: Option<AnyElement>,
        error: Option<String>,
        controls: Vec<AnyElement>,
    ) -> AnyElement {
        self.setting_card(id, glyph, title, desc, error, controls, None)
    }

    /// A setting with a part of its own below its title and description: a
    /// table of options, previews to choose from. The icon stays beside the
    /// title, as on the other cards, rather than in the middle of the card's
    /// height; the part lines up with the title.
    pub(in crate::frontend) fn setting_with_body(
        &self,
        id: &str,
        glyph: &str,
        title: &str,
        desc: &str,
        body: AnyElement,
    ) -> AnyElement {
        self.setting_card(
            id,
            self.glyph(glyph),
            label(title),
            Some(label(desc).into_any_element()),
            None,
            Vec::new(),
            Some(body),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::frontend) fn setting_card(
        &self,
        id: &str,
        glyph: AnyElement,
        title: impl IntoElement,
        desc: Option<AnyElement>,
        error: Option<String>,
        controls: Vec<AnyElement>,
        body: Option<AnyElement>,
    ) -> AnyElement {
        let p = self.palette;
        let tint = self.located_tint(id);
        let (border, fill) = self.located_colors(tint);
        let header = div()
            .flex()
            .items_center()
            .gap(px(16.))
            // A card is at least 68 high: this less its padding and border.
            .min_h(px(42.))
            .child(
                div()
                    .w(px(20.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(glyph),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(div().child(title))
                    .when_some(desc, |d, desc| {
                        d.child(
                            div()
                                .mt(px(2.))
                                .text_size(theme::font(12.))
                                .text_color(p.muted)
                                .child(desc),
                        )
                    })
                    .when_some(error, |d, error| {
                        d.child(
                            div()
                                .mt(px(4.))
                                .text_size(theme::font(12.))
                                .text_color(p.danger)
                                .child(error),
                        )
                    }),
            )
            .when(!controls.is_empty(), |d| {
                d.child(
                    div()
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .children(controls),
                )
            });
        div()
            .id(SharedString::from(format!("setting-{id}")))
            .flex()
            .flex_col()
            .px(px(16.))
            .py(px(12.))
            .border_1()
            .border_color(border)
            .rounded(px(6.))
            .bg(fill)
            .children(self.located_ring(tint))
            .child(header)
            .when_some(body, |card, body| {
                // Under the title: past the icon and the gap beside it.
                card.child(
                    div()
                        .pt(px(10.))
                        .pl(px(36.))
                        .text_size(theme::font(13.))
                        .text_color(p.ink)
                        .child(body),
                )
            })
            .into_any_element()
    }

    pub(in crate::frontend) fn setting_text(
        &self,
        id: &str,
        glyph: &str,
        title: &str,
        desc: &str,
        controls: Vec<AnyElement>,
    ) -> AnyElement {
        self.setting(
            id,
            self.glyph(glyph),
            label(title),
            Some(label(desc).into_any_element()),
            None,
            controls,
        )
    }

    /// One feature, one card (FeatureCard): what it is and its state on top;
    /// below a divider, what it reports beside what can be done.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn feature(
        &self,
        id: &str,
        glyph: &str,
        title: String,
        desc: String,
        state: String,
        tone: &str,
        note: AnyElement,
        error: Option<String>,
        controls: Vec<AnyElement>,
        footer: Option<String>,
    ) -> AnyElement {
        let p = self.palette;
        let tint = self.located_tint(id);
        let (border, fill) = self.located_colors(tint);
        div()
            .id(SharedString::from(format!("setting-{id}")))
            .flex()
            .flex_col()
            .border_1()
            .border_color(border)
            .rounded(px(6.))
            .bg(fill)
            .children(self.located_ring(tint))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(16.))
                    .px(px(16.))
                    .py(px(14.))
                    .child(div().w(px(20.)).flex_shrink_0().child(self.glyph(glyph)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(title)
                            .child(
                                div()
                                    .mt(px(2.))
                                    .text_size(theme::font(12.))
                                    .text_color(p.muted)
                                    .child(desc),
                            ),
                    )
                    .child(self.state_pill(state, tone)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(16.))
                    .min_h(px(52.))
                    .pl(px(52.))
                    .pr(px(16.))
                    .py(px(8.))
                    .border_t_1()
                    .border_color(p.stroke)
                    .when(id == "taskManager", |d| d.flex_col().items_start())
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .when(id == "taskManager", |d| d.w_full())
                            .text_size(theme::font(12.))
                            .text_color(p.muted)
                            .child(note)
                            .when_some(error, |d, error| {
                                d.child(div().mt(px(4.)).text_color(p.danger).child(error))
                            }),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .when(id == "taskManager", |d| d.w_full().flex_wrap())
                            .children(controls),
                    ),
            )
            .when_some(footer, |d, footer| {
                d.child(
                    div()
                        .pl(px(52.))
                        .pr(px(16.))
                        .pb(px(12.))
                        .text_size(theme::font(12.))
                        .text_color(p.faint)
                        .child(footer),
                )
            })
            .into_any_element()
    }

    /// A settings toggle running a command with `{argument: !value}`,
    /// named for assistive technology by the catalog text `title`: the title
    /// of the setting it switches.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::frontend) fn setting_toggle(
        &self,
        id: &str,
        title: &str,
        value: bool,
        command: &str,
        argument: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let command = command.to_owned();
        let argument = argument.to_owned();
        self.toggle(
            id.to_owned(),
            label(title),
            value,
            false,
            move |this, on, window, cx| {
                this.request(
                    &command,
                    json!({argument.as_str(): on}),
                    Some("settings"),
                    window,
                    cx,
                );
            },
            window,
            cx,
        )
    }

    /// A segmented choice that saves a command or a preference.
    pub(in crate::frontend) fn setting_choices(
        &self,
        key: &str,
        selected: &str,
        options: &[(&str, String)],
        command: Option<&str>,
        cx: &Context<Self>,
    ) -> AnyElement {
        self.segments(
            format!("setting-{key}"),
            options
                .iter()
                .map(|(value, name)| {
                    (
                        name.clone(),
                        None,
                        if let Some(command) = command {
                            Action::Invoke(command.into(), json!({key:value}))
                        } else {
                            Action::Preference(key.into(), json!(value))
                        },
                        selected == *value,
                    )
                })
                .collect(),
            cx,
        )
    }

    /// An inline text box kept across frames; Enter runs `submit`. The card
    /// draws its placeholder (`Desktop::text_field`).
    pub(super) fn inline_input(
        &mut self,
        key: &str,
        initial: &str,
        submit: fn(&mut Desktop, &mut Window, &mut Context<Desktop>),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        if let Some(input) = self.ui.inputs.get(key).cloned() {
            return input;
        }
        let initial = initial.to_owned();
        let input = cx.new(|cx| {
            let mut state = InputState::new(window, cx);
            state.set_value(initial, window, cx);
            state
        });
        let error_key = key.to_owned();
        self.subscriptions.push(cx.subscribe_in(
            &input,
            window,
            move |this, _, event, window, cx| match event {
                InputEvent::Change => {
                    this.ui.errors.remove(&error_key);
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => submit(this, window, cx),
                _ => {}
            },
        ));
        self.ui.inputs.insert(key.into(), input.clone());
        input
    }
}
