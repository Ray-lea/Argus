//! The process list's groups (users and categories): the colours of their
//! headings, the heading that stays at the top of the list while its group
//! scrolls under it, and a map of the groups on the scrollbar with the
//! group's name beside the thumb while the list scrolls.
//!
//! All three are drawn by the table's frame (`TableFrame`), over the list
//! and under the scrollbar the page draws: outside the list, so the list's
//! stretch past its ends (`ScrollBounce`) leaves them in place. The frame
//! renders again whenever the list does, so they follow every scrolled frame.
//! A wheel glide moves the list's offset on in the list's own prepaint, after
//! the frame has rendered: what follows the offset takes its place again in
//! its prepaint (`Follow`), or it would trail the rows by a frame.
use super::*;

/// The name's height.
const NAME_HEIGHT: f32 = 24.;
/// How long the group's name stays beside the scrollbar once scrolling stops,
/// and how long it then takes to fade.
const NAME_HOLD: Duration = Duration::from_secs(1);
const NAME_FADE: Duration = Duration::from_millis(180);
/// A heading's tint over the list, and of the count beside its title.
pub(super) const HEADING_TINT: f32 = 0.08;
pub(super) const COUNT_TINT: f32 = 0.18;

/// A heading's colour.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Tone {
    Apps,
    Background,
    Windows,
    /// The list's n-th user who is not the system.
    User(usize),
    System,
}

impl Tone {
    pub(super) fn color(self, p: &Palette) -> Hsla {
        match self {
            Tone::Apps => p.group_apps,
            Tone::Background => p.group_background,
            Tone::Windows | Tone::System => p.group_windows,
            Tone::User(n) => p.group_users[n % p.group_users.len()],
        }
    }

    fn key(self) -> String {
        match self {
            Tone::Apps => "apps".into(),
            Tone::Background => "background".into(),
            Tone::Windows => "windows".into(),
            Tone::User(n) => format!("user{}", n + 1),
            Tone::System => "system".into(),
        }
    }
}

/// A user's or a category's group: its heading, and the rows up to the next
/// heading of its level or above.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Group {
    pub(super) heading: usize,
    pub(super) end: usize,
    /// A category rather than a user's group.
    pub(super) category: bool,
    pub(super) tone: Tone,
}

/// The groups of `rows`, in order.
pub(super) fn groups(rows: &[Row]) -> Vec<Group> {
    let mut users = 0;
    let headings: Vec<(usize, bool, Tone)> = rows
        .iter()
        .enumerate()
        .filter_map(|(index, row)| {
            let section = row.section.as_deref()?;
            let tone = match row.user {
                Some(user) if user.system => Tone::System,
                Some(_) => {
                    users += 1;
                    Tone::User(users - 1)
                }
                None => match section {
                    "apps" => Tone::Apps,
                    "windows" => Tone::Windows,
                    _ => Tone::Background,
                },
            };
            Some((index, row.user.is_none(), tone))
        })
        .collect();
    headings
        .iter()
        .enumerate()
        .map(|(n, &(heading, category, tone))| Group {
            heading,
            // A category ends at any heading; a user's group at the next user's.
            end: headings[n + 1..]
                .iter()
                .find(|(_, next, _)| category || !next)
                .map_or(rows.len(), |next| next.0),
            category,
            tone,
        })
        .collect()
}

/// The headings held at the top of a list scrolled `scroll` down: the
/// user's, then the category's below it, each held while its group fills the
/// top and pushed up by the next heading. With how far down they reach.
fn held(groups: &[Group], count: usize, scroll: f32) -> (Vec<(usize, f32)>, f32) {
    let mut bands = Vec::new();
    let mut stack = 0.;
    if count == 0 {
        return (bands, 0.);
    }
    for category in [false, true] {
        let at = (((scroll + stack) / ROW_HEIGHT).floor() as usize).min(count - 1);
        let Some(group) = groups
            .iter()
            .find(|group| group.category == category && group.heading <= at && at < group.end)
        else {
            continue;
        };
        if group.heading as f32 * ROW_HEIGHT >= scroll + stack {
            continue;
        }
        let y = (group.end as f32 * ROW_HEIGHT - scroll - ROW_HEIGHT).min(stack);
        if y <= -ROW_HEIGHT {
            continue;
        }
        bands.push((group.heading, y));
        stack = y + ROW_HEIGHT;
    }
    let covered: f32 = stack;
    (bands, covered.max(0.))
}

/// Where the name beside the scrollbar goes: by the thumb, as the bar draws
/// it (`scrollbar::Geometry`), below the held headings.
fn name_top(scroll: f32, height: f32, max: f32, covered: f32) -> f32 {
    let total = height + max;
    let thumb = (height / total * height).round().max(17.).min(height);
    let middle = scroll / max.max(1.) * (height - thumb) + thumb / 2.;
    (middle - NAME_HEIGHT / 2.)
        .max(covered + 4.)
        .min(height - NAME_HEIGHT - 4.)
}

/// The scroll offset the list has this frame, once it has laid out.
fn scrolled(list: &ScrollHandle) -> f32 {
    f32::from(-list.offset().y).max(0.)
}

/// `child`, moved down by what `shift` says as it prepaints: after the list
/// has moved its offset on for this frame, where the render saw the last one.
struct Follow {
    child: AnyElement,
    shift: Box<dyn Fn() -> f32>,
}

impl IntoElement for Follow {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Follow {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.child.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let shift = (self.shift)();
        window.with_element_offset(point(px(0.), px(shift)), |window| {
            self.child.prepaint(window, cx)
        });
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.paint(window, cx);
    }
}

/// What the frame draws over the list this frame.
#[derive(Default)]
pub(super) struct Overlay {
    /// Headings held at the top, the user's before the category's, and how
    /// far down from the list's top each is.
    bands: Vec<(usize, f32)>,
    /// How far down the held headings reach.
    covered: f32,
    scroll: f32,
    height: f32,
    max: f32,
    /// The group the name beside the scrollbar names, and the row it is at.
    named: Option<(Group, usize)>,
    /// The name's opacity, while it shows.
    name: f32,
}

/// The process list's groups and what scrolling shows of them.
#[derive(Default)]
pub(super) struct GroupView {
    pub(super) groups: Rc<Vec<Group>>,
    overlay: Overlay,
    last_scroll: Option<f32>,
    scrolled: Option<Instant>,
    /// Hides the name once scrolling has stopped long enough.
    timer: Option<Task<()>>,
}

impl GroupView {
    /// The group headed by row `index`.
    pub(super) fn tone(&self, index: usize) -> Option<Tone> {
        self.groups
            .binary_search_by_key(&index, |group| group.heading)
            .ok()
            .map(|at| self.groups[at].tone)
    }

    /// The innermost group row `index` is in.
    fn group_of(&self, index: usize) -> Option<Group> {
        self.groups
            .iter()
            .rev()
            .find(|group| group.heading <= index && index < group.end)
            .copied()
    }

    /// Whether the pointer is over a held heading: the rows under it give no hints.
    pub(super) fn covers(&self, list: &ScrollHandle, window: &Window) -> bool {
        let viewport = list.bounds();
        let at = window.mouse_position();
        self.overlay.covered > 0.
            && viewport.contains(&at)
            && at.y < viewport.top() + px(self.overlay.covered)
    }

    pub(super) fn diagnostics(&self, rows: &[Row]) -> Value {
        let key = |index: usize| rows.get(index).map(|row| row.key.clone());
        json!({
            "groups": self.groups.iter().map(|group| json!({
                "key": key(group.heading),
                "tone": group.tone.key(),
                "rows": group.end - group.heading - 1,
            })).collect::<Vec<_>>(),
            "held": self.overlay.bands.iter().map(|(index, y)| json!({"key": key(*index), "y": y})).collect::<Vec<_>>(),
            "name": self.overlay.named.filter(|_| self.overlay.name > 0.).map(|(group, row)| json!({
                "key": key(group.heading),
                "index": row.saturating_sub(group.heading).max(1),
                "count": group.end - group.heading - 1,
            })),
        })
    }
}

impl NativeTable {
    /// Works out this frame's overlay from where the list is scrolled to.
    /// Called by the frame, before the table renders.
    pub(super) fn plan_groups(&mut self, window: &mut Window, cx: &mut Context<TableState<Self>>) {
        let view = &mut self.group_view;
        view.overlay = Overlay::default();
        let (Some(list), false, false) = (
            self.list.as_ref(),
            view.groups.is_empty(),
            self.rows.is_empty(),
        ) else {
            view.last_scroll = None;
            return;
        };
        let height = f32::from(list.bounds().size.height);
        if height <= 0. {
            return;
        }
        let scroll = scrolled(list);
        let count = self.rows.len();
        let (bands, covered) = held(&view.groups, count, scroll);
        let mut overlay = Overlay {
            bands,
            covered,
            scroll,
            height,
            max: f32::from(list.max_offset().y),
            ..Default::default()
        };
        // The name beside the scrollbar, from the first moved frame until a
        // second after the last.
        let now = Instant::now();
        if view
            .last_scroll
            .is_some_and(|last| (last - scroll).abs() > 0.5)
        {
            view.scrolled = Some(now);
        }
        view.last_scroll = Some(scroll);
        if let Some(scrolled) = view.scrolled {
            let since = now.saturating_duration_since(scrolled);
            let fade = if cx.reduce_motion() {
                Duration::ZERO
            } else {
                NAME_FADE
            };
            overlay.name = if since < NAME_HOLD {
                1.
            } else if since < NAME_HOLD + fade {
                window.request_animation_frame();
                1. - (since - NAME_HOLD).as_secs_f32() / fade.as_secs_f32()
            } else {
                view.scrolled = None;
                0.
            };
            if since < NAME_HOLD && view.timer.is_none() {
                let wait = NAME_HOLD - since + Duration::from_millis(5);
                view.timer = Some(cx.spawn(async move |table, cx| {
                    cx.background_executor().timer(wait).await;
                    let _ = table.update(cx, |table, cx| {
                        table.delegate_mut().group_view.timer = None;
                        cx.notify();
                    });
                }));
            }
        }
        if overlay.name > 0. {
            let row = (((scroll + overlay.covered) / ROW_HEIGHT).floor() as usize).min(count - 1);
            overlay.named = view.group_of(row).map(|group| (group, row));
        }
        view.overlay = overlay;
    }

    /// What the frame draws over the list, `top` down from the frame's top:
    /// the held headings, the groups on the scrollbar and the name beside it.
    pub(super) fn group_overlay(
        &mut self,
        top: Pixels,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> Option<AnyElement> {
        let p = Palette::current(cx);
        let overlay = &self.group_view.overlay;
        let (scroll, height, max) = (overlay.scroll, overlay.height, overlay.max);
        if height <= 0. || (overlay.bands.is_empty() && max < 0.5 && overlay.name <= 0.) {
            return None;
        }
        let bands = overlay.bands.clone();
        let last = bands.len().saturating_sub(1);
        let mut layer = div()
            .absolute()
            .left_0()
            .right_0()
            .top(top)
            .h(px(height))
            .overflow_hidden();
        for (n, (heading, y)) in bands.into_iter().enumerate() {
            let band = self.held_heading(heading, y, n == last, &p, window, cx);
            let (Some(list), groups, count) = (
                self.list.clone(),
                self.group_view.groups.clone(),
                self.rows.len(),
            ) else {
                layer = layer.child(band);
                continue;
            };
            // Where it is held now; out of sight once it no longer is.
            let shift = move || {
                held(&groups, count, scrolled(&list))
                    .0
                    .iter()
                    .find(|(at, _)| *at == heading)
                    .map_or(-y - 2. * ROW_HEIGHT, |(_, now)| now - y)
            };
            layer = layer.child(Follow {
                child: band,
                shift: Box::new(shift),
            });
        }
        // Over the held headings: they reach across the scrollbar's track,
        // where the first group's mark is, often shorter than a heading.
        if max >= 0.5 {
            layer = layer.child(self.group_marks(&p, scroll, height, max));
        }
        if let Some(name) = self.group_name(&p, scroll, height, max) {
            layer = layer.child(name);
        }
        Some(layer.into_any_element())
    }

    /// A heading held at the top, `y` down from the list's top: the heading
    /// row as the list draws it, on an opaque face so the rows under it
    /// stay hidden. A press on it folds the group, as on the heading.
    fn held_heading(
        &mut self,
        heading: usize,
        y: f32,
        lowest: bool,
        p: &Palette,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> AnyElement {
        let tone = self
            .group_view
            .tone(heading)
            .map_or(p.accent, |tone| tone.color(p));
        let key = self.rows[heading].key.clone();
        let owner = self.owner.clone();
        let offset = self
            .horizontal
            .as_ref()
            .map_or(px(0.), |handle| handle.offset().x);
        // Each cell as the table wraps it (`render_cell`): the columns have no padding.
        let cell = |width: Pixels, content: AnyElement| {
            div()
                .w(width)
                .h_full()
                .flex_shrink_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_sm()
                .child(content)
        };
        let name_width = self.columns.first().map_or(px(0.), |column| column.width);
        let name = self.td_content(heading, 0, window, cx);
        let mut lane = div().absolute().top_0().left(offset).h_full().flex();
        for column in 1..self.fields.len().min(self.columns.len()) {
            let width = self.columns[column].width;
            let content = self.td_content(heading, column, window, cx);
            lane = lane.child(cell(width, content));
        }
        div()
            .id(("held-heading", heading))
            .absolute()
            .left_0()
            .right_0()
            .top(px(y))
            .h(px(ROW_HEIGHT))
            .flex()
            .bg(p.chart_surface.blend(tone.opacity(HEADING_TINT)))
            // The rows under it neither light up nor take the press; the wheel still scrolls.
            .block_mouse_except_scroll()
            .child(cell(name_width, name))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .relative()
                    .overflow_hidden()
                    .child(lane),
            )
            .when(lowest, |d| {
                d.child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .bottom_0()
                        .h(px(1.))
                        .bg(p.stroke_strong),
                )
            })
            .on_click(move |_, window, cx| {
                let owner = owner.clone();
                let key = key.clone();
                window.defer(cx, move |window, cx| {
                    let _ = owner.update(cx, |this, cx| this.act(Action::Fold(key), window, cx));
                });
            })
            .into_any_element()
    }

    /// The groups along the scrollbar's track: each a line in its colour at
    /// the track's outer edge, clear of the thumb (3 px in from both sides,
    /// 2 px under the pointer), and fainter than it so the thumb stays the
    /// thing to find; the group at the top of the list stronger than the rest.
    fn group_marks(&self, p: &Palette, scroll: f32, height: f32, max: f32) -> AnyElement {
        let view = &self.group_view;
        let total = height + max;
        let current = view
            .group_of(((scroll + view.overlay.covered) / ROW_HEIGHT).floor() as usize)
            .map(|group| group.heading);
        // The innermost groups: a user's own when it has no categories.
        let leaves = view.groups.iter().filter(|group| {
            !view
                .groups
                .iter()
                .any(|inner| inner.heading > group.heading && inner.heading < group.end)
        });
        let mut track = div()
            .absolute()
            .top_0()
            .right_0()
            .w(px(scrollbar::SIZE))
            .h(px(height));
        for group in leaves {
            let top = group.heading as f32 * ROW_HEIGHT / total * height;
            let bottom = group.end as f32 * ROW_HEIGHT / total * height;
            let color = group.tone.color(p);
            track = track.child(
                div()
                    .absolute()
                    .right_0()
                    .w(px(2.))
                    .top(px(top + 1.))
                    .h(px((bottom - top - 2.).max(2.)))
                    .rounded(px(1.))
                    .bg(color.opacity(if Some(group.heading) == current {
                        0.75
                    } else {
                        0.3
                    })),
            );
        }
        track.into_any_element()
    }

    /// The group at the top of the list and the row the list is at in it,
    /// beside the scrollbar's thumb while the list scrolls.
    fn group_name(&self, p: &Palette, scroll: f32, height: f32, max: f32) -> Option<AnyElement> {
        let overlay = &self.group_view.overlay;
        let (group, row) = overlay.named.filter(|_| overlay.name > 0.)?;
        let title = &self.rows.get(group.heading)?.title;
        // A category in a user's group names the user too.
        let user = group
            .category
            .then(|| {
                self.group_view.groups.iter().find(|outer| {
                    !outer.category && outer.heading < group.heading && group.heading < outer.end
                })
            })
            .flatten()
            .and_then(|outer| self.rows.get(outer.heading));
        let name = match user {
            Some(user) => tr!(
                "processes.groupIn",
                user = user.title.clone(),
                group = title.clone()
            ),
            None => title.clone(),
        };
        let place = tr!(
            "processes.groupPlace",
            index = row.saturating_sub(group.heading).max(1),
            total = group.end - group.heading - 1
        );
        let top = name_top(scroll, height, max, overlay.covered);
        let (list, groups, count) = (
            self.list.clone(),
            self.group_view.groups.clone(),
            self.rows.len(),
        );
        let shift = move || {
            list.as_ref().map_or(0., |list| {
                let scroll = scrolled(list);
                let covered = held(&groups, count, scroll).1;
                name_top(scroll, height, max, covered) - top
            })
        };
        let color = group.tone.color(p);
        Some(
            div()
                .absolute()
                .right(px(scrollbar::SIZE + 6.))
                .top(px(top))
                .h(px(NAME_HEIGHT))
                .px(px(10.))
                .flex()
                .items_center()
                .gap(px(6.))
                .rounded_full()
                .bg(p.raised)
                .border_1()
                .border_color(p.stroke_strong)
                .text_size(theme::font(12.))
                .text_color(p.ink)
                .whitespace_nowrap()
                .opacity(overlay.name)
                .child(div().size(px(8.)).rounded(px(2.)).flex_shrink_0().bg(color))
                .child(name)
                .child(div().text_color(p.faint).child(place))
                .into_any_element(),
        )
        .map(|child| {
            Follow {
                child,
                shift: Box::new(shift),
            }
            .into_any_element()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{Group, Tone, groups};
    use crate::frontend::model::{Row, UserHeading};
    use serde_json::json;

    fn heading(section: &str, user: Option<UserHeading>) -> Row {
        let mut row = Row::item(json!({}), format!("s:{section}"), String::new());
        row.section = Some(section.into());
        row.user = user;
        row
    }

    fn process() -> Row {
        Row::item(json!({}), "p".into(), String::new())
    }

    #[test]
    fn a_category_ends_at_the_next_heading_and_a_user_at_the_next_user() {
        let human = UserHeading {
            system: false,
            current: true,
            disconnected: false,
        };
        let system = UserHeading {
            system: true,
            current: false,
            disconnected: false,
        };
        let rows = vec![
            heading("user", Some(human)),
            heading("apps", None),
            process(),
            process(),
            heading("background", None),
            process(),
            heading("user", Some(system)),
            heading("windows", None),
            process(),
        ];
        assert_eq!(
            groups(&rows),
            vec![
                Group {
                    heading: 0,
                    end: 6,
                    category: false,
                    tone: Tone::User(0)
                },
                Group {
                    heading: 1,
                    end: 4,
                    category: true,
                    tone: Tone::Apps
                },
                Group {
                    heading: 4,
                    end: 6,
                    category: true,
                    tone: Tone::Background
                },
                Group {
                    heading: 6,
                    end: 9,
                    category: false,
                    tone: Tone::System
                },
                Group {
                    heading: 7,
                    end: 9,
                    category: true,
                    tone: Tone::Windows
                },
            ]
        );
    }

    #[test]
    fn a_list_without_headings_has_no_groups() {
        assert!(groups(&[process(), process()]).is_empty());
    }
}
