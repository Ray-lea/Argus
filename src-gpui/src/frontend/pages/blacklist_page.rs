//! The blacklist page: the names Argus terminates as they appear, with the
//! add box and one remove button per row.
use super::*;

/// The column's width, as the settings page's content.
const CONTENT: f32 = 780.;

impl Desktop {
    pub(in crate::frontend) fn blacklist_page_view(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.palette;
        let names = blacklist::names(&self.prefs);
        let rows = names
            .iter()
            .enumerate()
            .map(|(index, name)| {
                let name = name.clone();
                div()
                    .id(("blacklist-row", index))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .px_3()
                    .py_1p5()
                    .when(index > 0, |d| d.border_t_1().border_color(p.stroke))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(theme::font(13.))
                            .child(name.clone()),
                    )
                    .child(
                        self.btn(
                            format!("blacklist-remove-{index}"),
                            label("blacklist.remove"),
                            None,
                            widgets::Tone::Plain,
                        )
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.act(Action::Blacklist(name.clone()), window, cx);
                            },
                        )),
                    )
            })
            .collect::<Vec<_>>();
        let add = self.blacklist_input.clone();
        div()
            .size_full()
            .min_h_0()
            .px(px(12.))
            .child(
                div()
                    .id("blacklist-content")
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.blacklist_scroll)
                    .flex()
                    .flex_col()
                    .items_center()
                    // The gutter is kept (`scrollbar-gutter: stable`).
                    .pr(px(scrollbar::SIZE))
                    .child(
                        div()
                            .w_full()
                            .max_w(px(CONTENT))
                            .pt(px(4.))
                            .pb(px(32.))
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            // What the page does, as a settings section's card.
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_2()
                                    .border_1()
                                    .border_color(p.stroke)
                                    .rounded(px(8.))
                                    .bg(p.layer)
                                    .px_4()
                                    .py_3()
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap_2()
                                            .text_size(theme::font(15.))
                                            .child(icon("ShieldIcon", p.accent).size_4())
                                            .child(label("blacklist.title")),
                                    )
                                    .child(
                                        div()
                                            .text_size(theme::font(13.))
                                            .text_color(p.muted)
                                            .child(label("blacklist.intro")),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .border_1()
                                    .border_color(p.stroke)
                                    .rounded(px(8.))
                                    .bg(p.layer)
                                    .overflow_hidden()
                                    // The add row, then the names.
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap_2()
                                            .px_3()
                                            .py_2()
                                            .border_b_1()
                                            .border_color(p.stroke)
                                            .child(div().flex_1().min_w_0().child(self.text_field(
                                                &add,
                                                Input::new(&add),
                                                widgets::Field::Boxed,
                                                label("blacklist.placeholder").into(),
                                                true,
                                                window,
                                                cx,
                                            )))
                                            .child(
                                                self.btn(
                                                    "blacklist-add-button",
                                                    label("blacklist.add"),
                                                    None,
                                                    widgets::Tone::Accent,
                                                )
                                                .on_click(cx.listener(|this, _, window, cx| {
                                                    let raw = this
                                                        .blacklist_input
                                                        .read(cx)
                                                        .value()
                                                        .to_string();
                                                    this.add_blacklist_name(&raw, window, cx);
                                                })),
                                            ),
                                    )
                                    .when(names.is_empty(), |d| {
                                        d.child(
                                            div()
                                                .px_3()
                                                .py_4()
                                                .text_size(theme::font(13.))
                                                .text_color(p.muted)
                                                .child(label("blacklist.empty")),
                                        )
                                    })
                                    .children(rows)
                                    .when(!names.is_empty(), |d| {
                                        d.child(
                                            div()
                                                .px_3()
                                                .py_1p5()
                                                .border_t_1()
                                                .border_color(p.stroke)
                                                .text_size(theme::font(12.))
                                                .text_color(p.muted)
                                                .child(tr!(
                                                    "blacklist.watchHint",
                                                    count = names.len()
                                                )),
                                        )
                                    }),
                            ),
                    )
                    .child(scrollbar::scrollbar(
                        "blacklist-scrollbar",
                        &self.blacklist_scroll,
                        p.dark,
                    )),
            )
            .into_any_element()
    }
}
