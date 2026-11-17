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
/// A link card's inset, and the size of its mark.
const INSET: f32 = 16.;
const MARK: f32 = 40.;

/// The pages each link card leads to besides its own address, with their
/// icons: what was there is reached in one step (`commands::project_page`).
const WEBSITE_PAGES: [(&str, &str); 3] = [
    ("download", "DownloadIcon"),
    ("guide", "GuideIcon"),
    ("changelog", "NotesIcon"),
];
const REPOSITORY_PAGES: [(&str, &str); 3] = [
    ("issues", "IssueIcon"),
    ("discussions", "DiscussionIcon"),
    ("releases", "ReleaseIcon"),
];

/// A link card's parts drawn for how far into its hover and press it is:
/// raised by 2 px and its face in, its line three times as long and its
/// arrow out by 2 px in the link's colour; a press lets it back down. Only
/// whole moves and fades: the mark and the text are never scaled, which
/// would draw them soft. `rise` is the 2 px rounded to whole device pixels,
/// so the raised card's text stays on the pixel grid.
fn lift_values(lift: &widgets::Lift, rise: f32) {
    let (hover, press) = (lift.hover.get(), lift.press.get());
    lift.card.set(LiveTransformValue {
        offset: point(px(0.), px(-rise * hover * (1. - press))),
        ..LiveTransformValue::default()
    });
    lift.face.set(LiveTransformValue {
        opacity: hover,
        ..LiveTransformValue::default()
    });
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
        let channel = text(&manifest["community"], "channel").to_owned();
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
            // The hundred dots: drawn by their own layer (`hundred`).
            .child(hundred::placeholder(
                self.hundred_place.clone(),
                self.page_epoch,
            ))
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
        cards.push(self.setting_text(
            "diagnostics",
            "DocumentIcon",
            "settings.about.diagnostics.title",
            "settings.about.diagnostics.desc",
            vec![
                self.btn(
                    "copy-diagnostics",
                    label("settings.about.diagnostics.copy"),
                    Some("CopyIcon"),
                    Tone::Plain,
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    let summary = this.diagnostic_summary(window, cx);
                    if this.options.fixture.is_some() {
                        // Fixture input must not overwrite the user's clipboard.
                        this.data.insert("diagnosticCopy".into(), json!(summary));
                    } else {
                        cx.write_to_clipboard(ClipboardItem::new_string(summary));
                    }
                    this.notify_toast_for(
                        ToastKind::Success,
                        tr!("actions.copied", what = label("settings.about.diagnostics.title")),
                        1800,
                    );
                    cx.notify();
                }))
                .into_any_element(),
            ],
        ));
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
        let channel_address = channel.trim_start_matches("https://").to_owned();
        cards.push(self.setting(
            "channel",
            self.glyph("ChannelIcon"),
            label("settings.about.channel.title"),
            Some(tr!("settings.about.channel.desc", channel = channel_address).into_any_element()),
            None,
            vec![
                self.btn(
                    "copy-channel",
                    label("settings.about.channel.copy"),
                    None,
                    Tone::Plain,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(channel.clone()));
                    this.notify_toast_for(
                        ToastKind::Success,
                        tr!("actions.copied", what = label("settings.about.channel.link")),
                        1800,
                    );
                    cx.notify();
                }))
                .into_any_element(),
                self.btn(
                    "channel",
                    label("settings.about.channel.join"),
                    None,
                    Tone::Accent,
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.request("open_channel", json!({}), None, window, cx)
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
    /// under GitHub's mark in ink. A whole card opens its address; the row
    /// along its foot opens the pages most visits are for.
    fn project_links(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = self.palette;
        let manifest = argus_app::config::manifest();
        let address = |url: &str| {
            url.trim_start_matches("https://")
                .trim_end_matches('/')
                .to_owned()
        };
        let website = text(manifest, "homepage");
        let website_mark = widgets::logo(MARK).into_any_element();
        let repository_mark = div()
            .size(px(MARK))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(9.))
            .bg(p.ink)
            // The page colour, opaque: the window's own background shows through Mica.
            .child(
                icon(
                    "GithubIcon",
                    widgets::solid(theme::token("window-bg", p.dark)),
                )
                .size(px(24.)),
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
                &WEBSITE_PAGES,
                cx,
            ))
            .child(self.project_link(
                "repository",
                repository_mark,
                address(crate::updater::REPO),
                p.ink,
                "open_repository",
                &REPOSITORY_PAGES,
                cx,
            ))
            .into_any_element()
    }

    /// One of the project's links: its mark beside its name and address in
    /// `tone`, the arrow that says it leaves the app, and along its foot the
    /// `pages` there, each a button of its own.
    #[allow(clippy::too_many_arguments)]
    fn project_link(
        &mut self,
        id: &'static str,
        mark: AnyElement,
        address: String,
        tone: Hsla,
        command: &'static str,
        pages: &[(&'static str, &'static str)],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use super::super::shift::{Anchor, live};
        let p = self.palette;
        let title = label(&format!("settings.about.links.{id}.title"));
        let fill = theme::mix(p.layer, tone, 0.06);
        let lifted = theme::mix(p.layer, tone, 0.12);
        let link = theme::mix(tone, p.ink, 0.25);
        // The address under the name: in the link's colour where it has
        // one, muted where the link is drawn in ink.
        let address_color = if tone == p.ink { p.muted } else { link };
        let lift = self
            .ui
            .lifts
            .entry(id)
            .or_insert_with(|| {
                let lift = widgets::Lift::default();
                lift_values(&lift, 2.);
                lift
            })
            .clone();
        let anchor = || Rc::new(Cell::new(Point::default()));
        let arrow = |color: Hsla| icon("ExternalIcon", color).size(px(14.));
        let anchors = self.ui.anchors.clone();
        let test = self.options.test;
        let head = div()
            .flex_1()
            .flex()
            .items_center()
            .gap(px(12.))
            .px(px(INSET))
            .pt(px(14.))
            .pb(px(12.))
            .child(div().flex_shrink_0().size(px(MARK)).child(mark))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    // Tall enough lines that the truncating boxes keep the
                    // descenders (g, y).
                    .line_height(relative(1.4))
                    .child(
                        div()
                            .truncate()
                            .font_family(theme::DISPLAY_FONT)
                            .text_size(theme::font(15.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(p.ink)
                            .child(title.clone()),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_size(theme::font(12.))
                            .font_weight(theme::LABEL_WEIGHT)
                            .text_color(address_color)
                            .child(address),
                    ),
            )
            // The arrow leans out on hover and takes the link's colour.
            .child(
                div()
                    .relative()
                    .flex_shrink_0()
                    .size(px(14.))
                    .child(live(
                        div().absolute().inset_0().child(arrow(p.muted)),
                        lift.arrow.clone(),
                        Anchor::TopLeft,
                        anchor(),
                    ))
                    .child(live(
                        div().absolute().inset_0().child(arrow(link)),
                        lift.lit.clone(),
                        Anchor::TopLeft,
                        anchor(),
                    )),
            );
        // The pages there: quiet buttons on a strip under a hairline. Their
        // press stays theirs (`btn` stops it), so the card neither sinks nor
        // opens its own address.
        let foot = div()
            .flex()
            .flex_wrap()
            .gap(px(2.))
            .px(px(INSET - 8.))
            .py(px(6.))
            .border_t_1()
            .border_color(tone.opacity(0.16))
            .children(pages.iter().map(|&(page, glyph)| {
                self.btn_small(
                    format!("about-link-{id}-{page}"),
                    label(&format!("settings.about.links.{id}.{page}")),
                    Some(glyph),
                    Tone::Ghost,
                )
                .role(Role::Link)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.request("open_project_page", json!({"page": page}), None, window, cx)
                }))
                .into_any_element()
            }));
        let card = gpui_kit::base::Button::new(SharedString::from(format!("about-link-{id}")))
            .relative()
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .items_stretch()
            .justify_start()
            .border_1()
            .border_color(tone.opacity(0.28))
            .rounded(px(8.))
            .bg(fill)
            .role(Role::Link)
            .accessibility_label(title)
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
                    .left(px(INSET))
                    .w(px(LINE))
                    .h(px(2.))
                    .rounded(px(1.))
                    .bg(tone),
                lift.line.clone(),
                Anchor::TopLeft,
                anchor(),
            ))
            .child(head)
            .child(foot)
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
        let scale = window.scale_factor();
        let rise = (2. * scale).round() / scale;
        if cx.reduce_motion() {
            self.live_motions.stop(&key);
            lift.hover.set(hover);
            lift.press.set(press);
            lift_values(&lift, rise);
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
                    lift_values(&stepped, rise);
                },
                |_| {},
            );
        }
        // A frame to begin on; the chart layer asks for the rest.
        window.refresh();
    }
}
