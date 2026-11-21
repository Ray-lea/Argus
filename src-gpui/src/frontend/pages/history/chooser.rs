//! The reading chooser: every reading there is to draw, found by typing or by
//! its group in the rail, each device's readings together under its name by
//! their own short names, so that many disks or adapters stay easy to scan.
//! It opens under the control that asked (a lane's reading, its overlay,
//! "More readings", the add-lane button) at a fixed size, its list
//! scrolling. A click chooses; the + beside a reading adds a lane for it.
//! Up and Down move through the readings, Enter chooses, Esc or a click
//! outside closes it.
use super::bar::ReadingMenu;
use super::readings::{Group, Reading};
use super::*;
use gpui_kit::component::input::{InputEvent, InputState};
use std::rc::Rc;

/// The most its list shows before it scrolls.
const LIST_MAX: f32 = 352.;
/// A reading's line, and how far a device's readings stand in.
const ROW: f32 = 30.;
const INDENT: f32 = 14.;
/// The search box, kept between openings (emptied each time).
const SEARCH: &str = "historyReadingSearch";

/// The chooser while open: what a choice does, and where it is.
pub(in crate::frontend) struct Chooser {
    title: Option<&'static str>,
    current: Option<String>,
    exclude: Vec<String>,
    pairs: bool,
    none: Option<Command>,
    add: bool,
    make: Rc<dyn Fn(String) -> Command>,
    anchor: String,
    /// The group the rail shows; none shows them all.
    group: Option<Group>,
    /// The choice the keys are on, among the lines that choose; none is the
    /// reading in force (else the first).
    highlight: Option<usize>,
    scroll: ScrollHandle,
}

/// A line of the list.
#[derive(Clone)]
enum Line {
    Group(Group),
    Device(String),
    /// No reading (an overlay taken off).
    None,
    Reading {
        id: String,
        label: String,
        color: Hsla,
        indent: bool,
        service: bool,
        recorded: bool,
    },
}

impl Line {
    fn chooses(&self) -> bool {
        matches!(self, Line::None | Line::Reading { .. })
    }
}

/// Whether `reading` answers a search for `query` (lower case): its names,
/// its device's, or its group's.
fn found(reading: &Reading, query: &str) -> bool {
    let group = reading.group.label();
    [
        Some(reading.long.as_str()),
        Some(reading.name.as_str()),
        reading.device.as_deref(),
        reading.part.as_deref(),
        Some(group.as_str()),
    ]
    .into_iter()
    .flatten()
    .any(|text| text.to_lowercase().contains(query))
}

impl Desktop {
    /// Opens the chooser under the control recorded as `anchor`: `make` turns
    /// the reading chosen into a command.
    pub(super) fn history_open_readings(
        &mut self,
        menu: ReadingMenu<'_>,
        make: impl Fn(String) -> Command + 'static,
        anchor: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = self.history_chooser_input(window, cx);
        input.update(cx, |state, cx| {
            state.set_value("", window, cx);
            state.focus(window, cx);
        });
        self.history.chooser = Some(Chooser {
            title: menu.title,
            current: menu.current.map(str::to_owned),
            exclude: menu.exclude.to_vec(),
            pairs: menu.pairs,
            none: menu.none,
            add: menu.add,
            make: Rc::new(make),
            anchor: anchor.to_owned(),
            group: None,
            highlight: None,
            scroll: ScrollHandle::new(),
        });
        cx.notify();
    }

    pub(super) fn history_close_chooser(&mut self, cx: &mut Context<Self>) {
        if self.history.chooser.take().is_some() {
            cx.notify();
        }
    }

    /// The search box, made once: typing filters the list and puts the keys
    /// on the first match, Enter chooses.
    fn history_chooser_input(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        if let Some(input) = self.ui.inputs.get(SEARCH).cloned() {
            return input;
        }
        let input = cx.new(|cx| InputState::new(window, cx));
        self.subscriptions.push(cx.subscribe_in(
            &input,
            window,
            |this, input, event, window, cx| match event {
                InputEvent::Change => {
                    let typed = !input.read(cx).value().trim().is_empty();
                    if let Some(chooser) = &mut this.history.chooser {
                        chooser.highlight = typed.then_some(0);
                    }
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => this.history_chooser_enter(window, cx),
                _ => {}
            },
        ));
        self.ui.inputs.insert(SEARCH.into(), input.clone());
        input
    }

    fn history_chooser_query(&self, cx: &App) -> String {
        self.ui
            .inputs
            .get(SEARCH)
            .map(|input| input.read(cx).value().trim().to_lowercase())
            .unwrap_or_default()
    }

    /// The readings that may be chosen, by group, before any search.
    fn history_chooser_readings(&self, chooser: &Chooser) -> Vec<&Reading> {
        self.history
            .readings
            .iter()
            .filter(|r| !chooser.exclude.contains(&r.id))
            .filter(|r| chooser.pairs || !r.is_pair())
            .filter(|r| r.recorded || r.service || r.group != Group::Battery)
            .collect()
    }

    /// The list: the group shown (each group, with a search or none chosen),
    /// its readings of no device first, then each device's under its name.
    fn history_chooser_lines(&self, chooser: &Chooser, query: &str) -> Vec<Line> {
        let searching = !query.is_empty();
        let readings = self.history_chooser_readings(chooser);
        let mut lines = Vec::new();
        if chooser.none.is_some() && !searching {
            lines.push(Line::None);
        }
        for group in Group::ALL {
            if !searching && chooser.group.is_some_and(|shown| shown != group) {
                continue;
            }
            let of_group = readings
                .iter()
                .filter(|r| r.group == group)
                .filter(|r| !searching || found(r, query))
                .collect::<Vec<_>>();
            if of_group.is_empty() {
                continue;
            }
            if searching || chooser.group.is_none() {
                lines.push(Line::Group(group));
            }
            let line = |r: &Reading, indent: bool| Line::Reading {
                id: r.id.clone(),
                label: if indent {
                    r.part.clone().unwrap_or_else(|| r.long.clone())
                } else {
                    r.long.clone()
                },
                color: r.color,
                indent,
                service: r.service,
                recorded: r.recorded,
            };
            lines.extend(
                of_group
                    .iter()
                    .filter(|r| r.device.is_none())
                    .map(|r| line(r, false)),
            );
            let mut devices: Vec<&str> = Vec::new();
            for device in of_group.iter().filter_map(|r| r.device.as_deref()) {
                if !devices.contains(&device) {
                    devices.push(device);
                }
            }
            for device in devices {
                lines.push(Line::Device(device.to_owned()));
                lines.extend(
                    of_group
                        .iter()
                        .filter(|r| r.device.as_deref() == Some(device))
                        .map(|r| line(r, true)),
                );
            }
        }
        lines
    }

    /// Which of the lines that choose the keys are on.
    fn history_chooser_highlight(chooser: &Chooser, lines: &[Line]) -> usize {
        chooser.highlight.unwrap_or_else(|| {
            lines
                .iter()
                .filter(|line| line.chooses())
                .position(|line| match line {
                    Line::Reading { id, .. } => chooser.current.as_deref() == Some(id.as_str()),
                    Line::None => chooser.current.is_none(),
                    _ => false,
                })
                .unwrap_or(0)
        })
    }

    /// Up or Down: the next choice that way, brought into view.
    pub(super) fn history_chooser_step(&mut self, by: isize, cx: &mut Context<Self>) {
        let query = self.history_chooser_query(cx);
        let Some(chooser) = self.history.chooser.as_ref() else {
            return;
        };
        let lines = self.history_chooser_lines(chooser, &query);
        let choices = lines.iter().filter(|line| line.chooses()).count();
        if choices == 0 {
            return;
        }
        let at = Self::history_chooser_highlight(chooser, &lines) as isize;
        let next = (at + by).clamp(0, choices as isize - 1) as usize;
        if let Some(position) = lines
            .iter()
            .enumerate()
            .filter(|(_, line)| line.chooses())
            .nth(next)
            .map(|(i, _)| i)
        {
            chooser.scroll.scroll_to_item(position);
        }
        if let Some(chooser) = &mut self.history.chooser {
            chooser.highlight = Some(next);
        }
        cx.notify();
    }

    /// Enter: the choice the keys are on.
    pub(super) fn history_chooser_enter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let query = self.history_chooser_query(cx);
        let Some(chooser) = self.history.chooser.as_ref() else {
            return;
        };
        let lines = self.history_chooser_lines(chooser, &query);
        let at = Self::history_chooser_highlight(chooser, &lines);
        if let Some(line) = lines.into_iter().filter(Line::chooses).nth(at) {
            self.history_choose(line, false, window, cx);
        }
    }

    /// Chooses `line` (with `add`, a new lane for it), closing the chooser.
    fn history_choose(
        &mut self,
        line: Line,
        add: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(chooser) = self.history.chooser.take() else {
            return;
        };
        let command = match line {
            Line::None => chooser.none.clone(),
            Line::Reading { id, .. } if add => Some(Command::AddLane(id)),
            Line::Reading { id, .. } => Some((chooser.make)(id)),
            _ => None,
        };
        if let Some(command) = command {
            self.history_command(command, window, cx);
        }
        cx.notify();
    }

    /// The chooser over the page, under the control it opened from.
    pub(super) fn history_chooser(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let chooser = self.history.chooser.as_ref()?;
        let input = self.ui.inputs.get(SEARCH).cloned()?;
        let p = self.palette;
        let query = self.history_chooser_query(cx);
        let searching = !query.is_empty();
        let lines = self.history_chooser_lines(chooser, &query);
        let highlight = Self::history_chooser_highlight(chooser, &lines);
        let viewport = window.viewport_size();
        let margin = px(8.);
        let anchor = self
            .ui
            .anchors
            .borrow()
            .get(chooser.anchor.as_str())
            .copied()
            .unwrap_or_default();
        // As wide as what it lists, in any language: the rail as its widest
        // group with its count, the list as its widest reading with its marks
        // and its + (over every reading, so that choosing a group or typing
        // leaves the width alone); a long device name wraps.
        let readings = self.history_chooser_readings(chooser);
        let measure = |text: &str, size: f32| {
            f32::from(theme::text_width(text, theme::font(size), window, cx))
        };
        let rail_width = std::iter::once(label("history.chooser.all"))
            .chain(Group::ALL.into_iter().map(Group::label))
            .map(|name| measure(&name, 13.) + 8. + measure("00", 11.5) + 20.)
            .fold(0., f32::max)
            + 8.
            + 1.;
        let list_width = readings
            .iter()
            .map(|r| {
                let (text, indent) = match (&r.device, &r.part) {
                    (Some(_), Some(part)) => (part.as_str(), INDENT),
                    _ => (r.long.as_str(), 0.),
                };
                let marks = match (r.service, r.recorded) {
                    (true, true) => 13. + 8.,
                    (true, false) => 13. + 8.,
                    (false, false) => 0.,
                    (false, true) => 0.,
                };
                10. + indent + 8. + 8. + measure(text, 13.) + 8. + marks + 16.
            })
            .chain(
                chooser
                    .none
                    .is_some()
                    .then(|| 10. + 16. + measure(&label("history.menu.none"), 13.) + 24.),
            )
            .fold(0., f32::max)
            + if chooser.add { 8. + 24. } else { 0. }
            + 4.
            + 8.
            + scrollbar::SIZE;
        let width = px(rail_width + list_width + 2.).min(viewport.width - margin * 2.);
        let left = anchor
            .left()
            .min(viewport.width - width - margin)
            .max(margin);
        // Under the control when it fits there, else over it.
        let tall = px(LIST_MAX + 90.);
        let below = anchor.bottom() + px(4.) + tall <= viewport.height - margin
            || anchor.top() - px(4.) - tall < margin;

        // The rail: every group, how many readings each has (that match).
        let count = |group: Option<Group>| {
            readings
                .iter()
                .filter(|r| group.is_none_or(|g| r.group == g))
                .filter(|r| !searching || found(r, &query))
                .count()
        };
        let rail_entry = |group: Option<Group>, cx: &mut Context<Self>| {
            let shown = !searching && chooser.group == group;
            let n = count(group);
            let name = group.map_or_else(|| label("history.chooser.all"), Group::label);
            let id = SharedString::from(format!(
                "history-chooser-group-{}",
                group.map_or("all".into(), |g| format!("{g:?}").to_lowercase())
            ));
            div()
                .id(id)
                .flex()
                .items_center()
                .gap_2()
                .h(px(28.))
                .px(px(10.))
                .rounded(px(5.))
                .cursor_pointer()
                .when(shown, |d| d.bg(p.control).text_color(p.ink))
                .when(!shown, |d| {
                    d.text_color(p.muted).hover(move |s| s.bg(p.hover))
                })
                .when(n == 0, |d| d.opacity(0.5))
                .child(div().flex_1().min_w_0().truncate().child(name))
                .child(
                    div()
                        .flex_shrink_0()
                        .text_size(theme::font(11.5))
                        .text_color(p.faint)
                        .font_features(theme::tabular())
                        .child(n.to_string()),
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    // A group chosen while searching shows it whole.
                    if let Some(input) = this.ui.inputs.get(SEARCH).cloned() {
                        input.update(cx, |state, cx| state.set_value("", window, cx));
                    }
                    if let Some(chooser) = &mut this.history.chooser {
                        chooser.group = group;
                        chooser.highlight = None;
                        chooser.scroll.set_offset(point(px(0.), px(0.)));
                    }
                    cx.notify();
                }))
        };
        let mut rail = div()
            .flex_shrink_0()
            .w(px(rail_width))
            .p(px(4.))
            .flex()
            .flex_col()
            .gap(px(1.))
            .border_r_1()
            .border_color(p.stroke)
            .child(rail_entry(None, cx));
        for group in Group::ALL {
            if count(Some(group)) > 0 || readings.iter().any(|r| r.group == group) {
                rail = rail.child(rail_entry(Some(group), cx));
            }
        }

        // The list.
        let mut list = div()
            .id("history-chooser-list")
            .track_scroll(&chooser.scroll)
            .flex_1()
            .min_w_0()
            .overflow_y_scroll()
            .p(px(4.))
            .flex()
            .flex_col();
        if lines.is_empty() {
            list = list.child(
                div()
                    .px(px(10.))
                    .py(px(12.))
                    .text_color(p.faint)
                    .child(label("history.chooser.empty")),
            );
        }
        let mut choice = 0;
        for (index, line) in lines.into_iter().enumerate() {
            match &line {
                Line::Group(group) => {
                    list = list.child(
                        div()
                            .flex_shrink_0()
                            .px(px(10.))
                            .pt(px(if index == 0 { 4. } else { 10. }))
                            .pb(px(3.))
                            .text_size(theme::font(11.5))
                            .text_color(p.faint)
                            .child(group.label()),
                    );
                    continue;
                }
                Line::Device(name) => {
                    // Wrapped when longer than the readings under it.
                    list = list.child(
                        div()
                            .flex_shrink_0()
                            .px(px(10.))
                            .pt(px(6.))
                            .pb(px(2.))
                            .text_color(p.muted)
                            .font_weight(theme::LABEL_WEIGHT)
                            .child(name.clone()),
                    );
                    continue;
                }
                _ => {}
            }
            let keyed = choice == highlight;
            choice += 1;
            let (label_text, color, indent, service, recorded, id) = match &line {
                Line::Reading {
                    id,
                    label,
                    color,
                    indent,
                    service,
                    recorded,
                } => (
                    label.clone(),
                    Some(*color),
                    *indent,
                    *service,
                    *recorded,
                    id.clone(),
                ),
                _ => (
                    label("history.menu.none"),
                    None,
                    false,
                    false,
                    true,
                    String::new(),
                ),
            };
            let current = match &line {
                Line::Reading { id, .. } => chooser.current.as_deref() == Some(id.as_str()),
                _ => chooser.current.is_none(),
            };
            let chosen = line.clone();
            let mut row = div()
                .id(SharedString::from(format!("history-chooser-{index}")))
                .flex_shrink_0()
                .flex()
                .items_center()
                .gap(px(8.))
                .h(px(ROW))
                .pl(px(10. + if indent { INDENT } else { 0. }))
                .pr(px(4.))
                .rounded(px(5.))
                .cursor_pointer()
                .when(keyed, |d| d.bg(p.hover))
                .hover(move |s| s.bg(p.hover))
                .child(
                    div()
                        .size(px(8.))
                        .flex_shrink_0()
                        .rounded(px(2.))
                        .when_some(color, |d, color| d.bg(color)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(if recorded { p.ink } else { p.muted })
                        .child(label_text),
                );
            // Whether the privileged service reads it, and whether anything
            // was recorded: a mark, and its words on hover.
            let missing = if service && !recorded {
                Some("history.menu.needsService")
            } else if !recorded {
                Some("history.menu.notRecorded")
            } else {
                None
            };
            if service {
                row = row.child(
                    icon(
                        if recorded {
                            "ShieldCheckIcon"
                        } else {
                            "ShieldAlertIcon"
                        },
                        if recorded { p.faint } else { p.io },
                    )
                    .size(px(13.)),
                );
            }
            if let Some(missing) = missing {
                row = row.tooltip(move |window, cx| widgets::tooltip(label(missing), window, cx));
            }
            row = row.child(div().w(px(16.)).flex_shrink_0().when(current, |d| {
                d.child(icon("CheckIcon", p.accent).size(px(14.)))
            }));
            if chooser.add && !id.is_empty() {
                let added = chosen.clone();
                row = row.child(
                    div()
                        .id(SharedString::from(format!("history-chooser-add-{index}")))
                        .size(px(24.))
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(4.))
                        .hover(move |s| s.bg(p.hover_strong))
                        .child(icon("PlusIcon", p.muted).size(px(13.)))
                        .tooltip(|window, cx| {
                            widgets::tooltip(label("history.tab.add"), window, cx)
                        })
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.history_choose(added.clone(), true, window, cx);
                        })),
                );
            }
            list = list.child(row.on_click(cx.listener(move |this, _, window, cx| {
                this.history_choose(chosen.clone(), false, window, cx);
            })));
        }

        let head = div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .px(px(10.))
            .pt(px(10.))
            .pb(px(8.))
            .border_b_1()
            .border_color(p.stroke)
            .when_some(chooser.title, |d, title| {
                d.child(
                    div()
                        .text_size(theme::font(12.))
                        .text_color(p.muted)
                        .child(label(title)),
                )
            })
            .child(self.text_field(
                &input,
                widgets::input(&input),
                widgets::Field::Boxed,
                label("history.chooser.search").into(),
                true,
                window,
                cx,
            ));
        let panel = div()
            .id("history-chooser")
            .absolute()
            .left(left)
            .when(below, |d| d.top(anchor.bottom() + px(4.)))
            .when(!below, |d| {
                d.bottom(viewport.height - anchor.top() + px(4.))
            })
            .w(width)
            .flex()
            .flex_col()
            .border_1()
            .border_color(p.stroke_strong)
            .rounded(px(8.))
            .bg(widgets::solid(p.flyout))
            .text_size(theme::font(13.))
            .text_color(p.ink)
            .line_height(relative(1.4))
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .on_scroll(|_, _, cx| cx.stop_propagation())
            .child(head)
            .child(
                div()
                    .flex()
                    .max_h(px(LIST_MAX))
                    .min_h(px(200.))
                    .child(rail)
                    .child(list.child(div().h(px(4.)).flex_shrink_0())),
            );
        Some(
            deferred(
                anchored().position(point(px(0.), px(0.))).child(
                    div()
                        .id("history-chooser-layer")
                        .w(viewport.width)
                        .h(viewport.height)
                        .occlude()
                        // A press anywhere else closes it.
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| this.history_close_chooser(cx)),
                        )
                        .on_mouse_down(
                            MouseButton::Right,
                            cx.listener(|this, _, _, cx| this.history_close_chooser(cx)),
                        )
                        .on_scroll(|_, _, cx| cx.stop_propagation())
                        .child(panel),
                ),
            )
            .with_priority(gpui_kit::base::POPUP_PRIORITY - 1)
            .into_any_element(),
        )
    }

    /// What the tests read of the chooser.
    pub(super) fn history_chooser_state(&self, cx: &App) -> Value {
        let Some(chooser) = self.history.chooser.as_ref() else {
            return Value::Null;
        };
        let query = self.history_chooser_query(cx);
        let lines = self.history_chooser_lines(chooser, &query);
        let highlight = Self::history_chooser_highlight(chooser, &lines);
        json!({
            "open": true,
            "group": chooser.group.map(|g| format!("{g:?}").to_lowercase()),
            "choices": lines.iter().filter(|line| line.chooses()).count(),
            "devices": lines.iter().filter(|line| matches!(line, Line::Device(_))).count(),
            "highlight": highlight,
        })
    }
}
