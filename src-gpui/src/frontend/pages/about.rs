//! About: the app and this build (its version, permissions and the notes of
//! what it changed), then where to find help. A page of its own in the
//! navigation, after Settings.
use super::*;
use crate::frontend::widgets::{ToastKind, Tone, service_connected};

/// The column's width, as the settings page's content.
const CONTENT: f32 = 780.;

/// A link card's hover (the interface's `t-med`, decelerating) and press.
const HOVER: Duration = Duration::from_millis(167);
const PRESS: Duration = Duration::from_millis(100);
/// The line along a link card's top edge, and how much longer hover draws it.
const LINE: f32 = 32.;
const LINE_HOVER: f32 = 3.;

/// A link card's parts drawn for how far into its hover and press it is:
/// raised by 2 px and its face in, its mark 6 % larger, its line three times
/// as long and its arrow out by 2 px in the link's colour; a press sets it
/// 1.5 % deeper.
fn lift_values(lift: &widgets::Lift) {
    let (hover, press) = (lift.hover.get(), lift.press.get());
    let scale = 1. - 0.015 * press;
    lift.card.set(LiveTransformValue {
        offset: point(px(0.), px(-2. * hover * (1. - press))),
        scale_x: scale,
        scale_y: scale,
        ..LiveTransformValue::default()
    });
    lift.face.set(LiveTransformValue {
        opacity: hover,
        ..LiveTransformValue::default()
    });
    lift.mark
        .set(LiveTransformValue::zoom(1. + 0.06 * hover, 1.));
    lift.line.set(LiveTransformValue {
        scale_x: 1. + (LINE_HOVER - 1.) * hover,
        ..LiveTransformValue::default()
    });
    let out = point(px(2. * hover), px(-2. * hover));
    lift.arrow.set(LiveTransformValue {
        offset: out,
        opacity: 1. - hover,
        ..LiveTransformValue::default()
    });
    lift.lit.set(LiveTransformValue {
        offset: out,
        opacity: hover,
        ..LiveTransformValue::default()
    });
}

/// Figures that keep their width (`tnum`).
fn tabular() -> FontFeatures {
    FontFeatures(Arc::new(vec![("tnum".into(), 1)]))
}

impl Desktop {
    /// About asks once on arrival whether the privileged service is
    /// connected (the overview says so).
    pub(in crate::frontend) fn poll_about(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let epoch = self.page_epoch.to_string();
        if self.ui.errors.get("aboutEpoch") != Some(&epoch) {
            self.ui.errors.insert("aboutEpoch".into(), epoch);
            self.request(
                "privileged_service_status",
                json!({}),
                Some("privilege"),
                window,
                cx,
            );
        }
    }

    pub(in crate::frontend) fn about_view(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.palette;
        div()
            .size_full()
            .min_h_0()
            .px(px(12.))
            .child(
                div()
                    .id("about-content")
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.about_scroll)
                    .flex()
                    .items_start()
                    .justify_center()
                    // The gutter is kept (`scrollbar-gutter: stable`).
                    .pr(px(scrollbar::SIZE))
                    .child(
                        div()
                            .w_full()
                            .max_w(px(CONTENT))
                            .pt(px(4.))
                            .pb(px(32.))
                            .child(self.about_overview(cx)),
                    )
                    .child(scrollbar::scrollbar(
                        "about-scrollbar",
                        &self.about_scroll,
                        p.dark,
                    )),
            )
            .into_any_element()
    }

    /// The app and this build, then where to find help.
    fn about_overview(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = self.palette;
        let manifest = argus_app::config::manifest();
        let privilege = self.data.get("privilege").cloned().unwrap_or(Value::Null);
        let group = text(&manifest["community"], "group").to_owned();
        let line = |text: String| {
            div()
                .mt(px(2.))
                .text_size(theme::font(12.))
                .text_color(p.muted)
                .child(text)
        };
        let hero = div()
            .id("setting-about")
            .mt(px(6.))
            .flex()
            .items_center()
            .gap(px(20.))
            .min_h(px(112.))
            .px(px(20.))
            .py(px(16.))
            .border_1()
            .border_color(p.stroke)
            .rounded(px(8.))
            .bg(p.layer)
            .child(widgets::logo(64.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_baseline()
                            .gap_x(px(10.))
                            .child(
                                div()
                                    .font_family(theme::DISPLAY_FONT)
                                    .text_size(theme::font(24.))
                                    .font_weight(theme::LABEL_WEIGHT)
                                    .child(argus_app::config::product_name()),
                            )
                            .child(
                                div()
                                    .text_size(theme::font(13.))
                                    .text_color(p.muted)
                                    .font_features(tabular())
                                    .child(format!(
                                        "{} · {}",
                                        self.shown_version(),
                                        self.build_label()
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .mt(px(4.))
                            .text_size(theme::font(13.))
                            .child(label("settings.about.tagline")),
                    )
                    .child(line(label(if boolean(&self.data["info"], "elevated") {
                        "settings.about.admin"
                    } else if service_connected(&privilege) {
                        "settings.about.service"
                    } else {
                        "settings.about.standard"
                    })))
                    .child(line(format!("© 2026 {}", env!("CARGO_PKG_AUTHORS")))),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .flex()
                    .gap(px(8.))
                    .child(
                        self.btn(
                            "about-whats-new",
                            label("settings.about.whatsNew"),
                            None,
                            Tone::Plain,
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.open_dialog("whatsnew", json!({}), window, cx)
                        })),
                    )
                    .child(
                        self.btn(
                            "about-quit",
                            label("settings.about.quit"),
                            None,
                            Tone::Plain,
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.request("quit", json!({}), None, window, cx)
                        })),
                    ),
            );
        let mut cards = Vec::new();
        let copy_group = group.clone();
        cards.push(self.setting(
            "community",
            self.glyph("ChatIcon"),
            label("settings.about.community.title"),
            Some(tr!("settings.about.community.desc", group = group).into_any_element()),
            None,
            vec![
                self.btn(
                    "copy-community",
                    label("settings.about.community.copy"),
                    None,
                    Tone::Plain,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(copy_group.clone()));
                    this.notify_toast_for(
                        ToastKind::Success,
                        tr!(
                            "actions.copied",
                            what = label("settings.about.community.number")
                        ),
                        1800,
                    );
                    cx.notify();
                }))
                .into_any_element(),
                self.btn(
                    "community",
                    label("settings.about.community.join"),
                    None,
                    Tone::Accent,
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.request("open_community", json!({}), None, window, cx)
                }))
                .into_any_element(),
            ],
        ));
        cards.push(self.setting_text(
            "onboarding",
            "SparkleIcon",
            "settings.about.onboarding.title",
            "settings.about.onboarding.desc",
            vec![
                self.btn(
                    "onboarding",
                    label("settings.about.onboarding.open"),
                    None,
                    Tone::Plain,
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.open_dialog("onboarding", json!({"step":0}), window, cx)
                }))
                .into_any_element(),
            ],
        ));
        for (id, key) in [
            ("license", "settings.about.license.view"),
            ("notices", "settings.about.notices.view"),
        ] {
            cards.push(self.setting_text(
                id,
                "DocumentIcon",
                &format!("settings.about.{id}.title"),
                &format!("settings.about.{id}.desc"),
                vec![
                    self.btn(id, label(key), None, Tone::Plain)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.open_dialog(id, json!({}), window, cx)
                        }))
                        .into_any_element(),
                ],
            ));
        }
        cards.push(self.setting_text(
            "log",
            "StorageIcon",
            "settings.about.log.title",
            "settings.about.log.desc",
            vec![
                self.btn("log", label("settings.about.log.open"), None, Tone::Plain)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.request("open_log_dir", json!({}), None, window, cx)
                    }))
                    .into_any_element(),
            ],
        ));
        div()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .gap(px(4.))
            .child(hero)
            .child(self.project_links(cx))
            .child(
                div()
                    .mt(px(24.))
                    .mb(px(6.))
                    .text_size(theme::font(14.))
                    .font_weight(theme::LABEL_WEIGHT)
                    .child(label("settings.about.support")),
            )
            .children(cards)
            .into_any_element()
    }

    /// The project's two places on the web, side by side, each in its own
    /// colours: the website under the app's mark and accent, the repository
    /// under GitHub's mark in ink. A whole card opens its address.
    fn project_links(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = self.palette;
        let manifest = argus_app::config::manifest();
        let address = |url: &str| {
            url.trim_start_matches("https://")
                .trim_end_matches('/')
                .to_owned()
        };
        let website = text(manifest, "homepage");
        let website_mark = widgets::logo(44.).into_any_element();
        let repository_mark = div()
            .size(px(44.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(10.))
            .bg(p.ink)
            // The page colour, opaque: the window's own background shows through Mica.
            .child(
                icon(
                    "GithubIcon",
                    widgets::solid(theme::token("window-bg", p.dark)),
                )
                .size(px(26.)),
            )
            .into_any_element();
        div()
            .mt(px(8.))
            .grid()
            .grid_cols(2)
            .gap(px(8.))
            .child(self.project_link(
                "website",
                website_mark,
                address(website),
                p.accent,
                "open_homepage",
                cx,
            ))
            .child(self.project_link(
                "repository",
                repository_mark,
                address(crate::updater::REPO),
                p.ink,
                "open_repository",
                cx,
            ))
            .into_any_element()
    }

    /// One of the project's links: its mark, name and address in `tone`, what
    /// is there, and the arrow that says it leaves the app.
    fn project_link(
        &mut self,
        id: &'static str,
        mark: AnyElement,
        address: String,
        tone: Hsla,
        command: &'static str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use super::super::shift::{Anchor, live};
        let p = self.palette;
        let title = label(&format!("settings.about.links.{id}.title"));
        let fill = theme::mix(p.layer, tone, 0.06);
        let lifted = theme::mix(p.layer, tone, 0.12);
        let lift = self
            .ui
            .lifts
            .entry(id)
            .or_insert_with(|| {
                let lift = widgets::Lift::default();
                lift_values(&lift);
                lift
            })
            .clone();
        let anchor = || Rc::new(Cell::new(Point::default()));
        let arrow = |color: Hsla| icon("ExternalIcon", color).size(px(15.));
        let anchors = self.ui.anchors.clone();
        let test = self.options.test;
        let card = gpui_kit::base::Button::new(SharedString::from(format!("about-link-{id}")))
            .relative()
            .w_full()
            .min_w_0()
            .min_h(px(132.))
            .flex()
            .flex_col()
            .items_start()
            .justify_start()
            .gap(px(2.))
            .line_height(relative(1.45))
            .px(px(18.))
            .pt(px(16.))
            .pb(px(16.))
            .border_1()
            .border_color(tone.opacity(0.28))
            .rounded(px(8.))
            .bg(fill)
            .role(Role::Link)
            .accessibility_label(title.clone())
            // The hover face, fading in over the card's own: a stronger
            // fill and edge in the link's colour.
            .child(live(
                div()
                    .absolute()
                    .top(px(-1.))
                    .bottom(px(-1.))
                    .left(px(-1.))
                    .right(px(-1.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(tone.opacity(0.55))
                    .bg(lifted),
                lift.face.clone(),
                Anchor::TopLeft,
                anchor(),
            ))
            // A line of the link's colour along the top edge, longer on hover.
            .child(live(
                div()
                    .absolute()
                    .top_0()
                    .left(px(18.))
                    .w(px(LINE))
                    .h(px(2.))
                    .rounded(px(1.))
                    .bg(tone),
                lift.line.clone(),
                Anchor::TopLeft,
                anchor(),
            ))
            // The arrow leans out on hover and takes the link's colour.
            .child(
                div()
                    .absolute()
                    .top(px(14.))
                    .right(px(14.))
                    .size(px(15.))
                    .child(live(
                        div().absolute().inset_0().child(arrow(p.muted)),
                        lift.arrow.clone(),
                        Anchor::TopLeft,
                        anchor(),
                    ))
                    .child(live(
                        div()
                            .absolute()
                            .inset_0()
                            .child(arrow(theme::mix(tone, p.ink, 0.25))),
                        lift.lit.clone(),
                        Anchor::TopLeft,
                        anchor(),
                    )),
            )
            .child(div().mb(px(10.)).child(live(
                div().child(mark),
                lift.mark.clone(),
                Anchor::Center,
                anchor(),
            )))
            .child(
                div()
                    .font_family(theme::DISPLAY_FONT)
                    .text_size(theme::font(16.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(p.ink)
                    .child(title),
            )
            .child(
                div()
                    .whitespace_nowrap()
                    .text_size(theme::font(12.5))
                    .font_weight(theme::LABEL_WEIGHT)
                    .text_color(theme::mix(tone, p.ink, 0.25))
                    .child(address),
            )
            .child(
                div()
                    .mt(px(2.))
                    .text_size(theme::font(12.))
                    .text_color(p.muted)
                    .whitespace_normal()
                    .text_left()
                    .child(label(&format!("settings.about.links.{id}.desc"))),
            )
            // Where tests take hold of it.
            .when(test, |d| {
                d.child(
                    canvas(
                        move |bounds, _, _| {
                            anchors
                                .borrow_mut()
                                .insert(format!("about-link-{id}").into(), bounds);
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .inset_0(),
                )
            })
            .on_hover(cx.listener(move |this, hovered: &bool, window, cx| {
                this.lift(id, Some(*hovered), None, window, cx)
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, window, cx| this.lift(id, None, Some(true), window, cx)),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(move |this, _, window, cx| {
                    this.lift(id, None, Some(false), window, cx)
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(move |this, _, window, cx| {
                    this.lift(id, None, Some(false), window, cx)
                }),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.request(command, json!({}), None, window, cx)
            }));
        live(card, lift.card, Anchor::Center, anchor()).into_any_element()
    }

    /// Eases a link card toward hovered or not, pressed or not, from where
    /// it is drawn now; under reduced motion it goes there at once.
    fn lift(
        &mut self,
        id: &'static str,
        hovered: Option<bool>,
        pressed: Option<bool>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(lift) = self.ui.lifts.get_mut(id) else {
            return;
        };
        if let Some(hovered) = hovered {
            lift.hovered = hovered;
            // Leaving lets go of a press.
            lift.pressed &= hovered;
        }
        if let Some(pressed) = pressed {
            lift.pressed = pressed;
        }
        let lift = lift.clone();
        let (hover, press) = (f32::from(lift.hovered), f32::from(lift.pressed));
        let key = format!("about-link-{id}");
        if cx.reduce_motion() {
            self.live_motions.stop(&key);
            lift.hover.set(hover);
            lift.press.set(press);
            lift_values(&lift);
        } else {
            let (from_hover, from_press) = (lift.hover.get(), lift.press.get());
            let stepped = lift.clone();
            self.live_motions.start(
                key,
                HOVER,
                move |elapsed| {
                    let ease = |span: Duration| {
                        theme::ease_out((elapsed.as_secs_f32() / span.as_secs_f32()).min(1.))
                    };
                    stepped
                        .hover
                        .set(from_hover + (hover - from_hover) * ease(HOVER));
                    stepped
                        .press
                        .set(from_press + (press - from_press) * ease(PRESS));
                    lift_values(&stepped);
                },
                |_| {},
            );
        }
        // A frame to begin on; the chart layer asks for the rest.
        window.refresh();
    }
}
