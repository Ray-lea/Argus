//! The lanes' tooltip: the moment or bucket under the pointer, the lane's
//! readings there, and what made them, by what the reading is made of
//! (`TipList`): the processes busiest by the reading's share of each, the
//! programs with the most processes for the process count, or nothing.
use super::*;

/// The processes the lanes' tooltip lists; it keeps their rows while fewer
/// are known, so that it keeps its height.
const TIP_PROCESSES: usize = 5;
/// The lanes' tooltip: its width, fixed so that it keeps its size and place
/// as the pointer passes readings and process names of other lengths; how
/// far it sits from the pointer; and the room it keeps from the window's edges.
const TIP_WIDTH: f32 = 300.;
const TIP_GAP: f32 = 14.;
const TIP_MARGIN: f32 = 8.;

/// What a lane's tooltip lists under its readings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum TipList {
    /// The processes busiest by the reading's share of each
    /// (`table::process_value`).
    Processes(&'static str),
    /// The programs with the most processes (the process count).
    Programs,
    /// Nothing: no reading of each process adds up to it (a battery's
    /// charge, a temperature).
    #[default]
    Nothing,
}

impl Desktop {
    /// The trend column follows the topmost lane's reading.
    /// The process value that goes with a lane's reading (the tooltip ranks by it).
    /// What a lane's tooltip lists: the processes by the reading's share of
    /// each, the programs by their processes for the process count, nothing
    /// for readings no process adds to (a battery's charge, a temperature):
    /// ranked by CPU instead, they read as their cause.
    fn history_tip_list(&self, lane: &state::Lane) -> TipList {
        match self.history_reading(&lane.reading) {
            Some(reading) if reading.id == "processes" => TipList::Programs,
            Some(reading) => reading.process.map_or(TipList::Nothing, TipList::Processes),
            None => TipList::Nothing,
        }
    }

    /// The five processes busiest by `key` at point `index`, by name, once
    /// their answer for that point is in.
    /// The processes the tooltip lists, busiest by `key` first: the point's
    /// under the pointer, or while they are asked for, the last point's. The
    /// list keeps its rows as the pointer moves, rather than emptying for a
    /// moment at every point.
    fn history_tip_ranking(&self, key: &str) -> Vec<(String, f64)> {
        let Some((_, answer)) = &self.history.tip else {
            return Vec::new();
        };
        // A range answer holds totals over the time recorded.
        let seconds = answer["recorded"].as_f64().map(|ms| ms / 1000.);
        let mut by_name: Vec<(String, f64)> = Vec::new();
        for process in values(&answer["processes"]) {
            let Some(value) = table::process_value(process, key, seconds) else {
                continue;
            };
            let name = text(process, "name").to_owned();
            match by_name
                .iter_mut()
                .find(|(n, _)| n.eq_ignore_ascii_case(&name))
            {
                Some((_, sum)) => *sum += value,
                None => by_name.push((name, value)),
            }
        }
        by_name.retain(|(_, v)| *v > 0.);
        by_name.sort_by(|a, b| b.1.total_cmp(&a.1));
        by_name.truncate(TIP_PROCESSES);
        by_name
    }

    /// Asks for the processes of the point under the pointer: its frame, or for
    /// a bucket the range table over it.
    pub(super) fn history_ask_tip(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(hover) = self.history.hover else {
            return;
        };
        let Some(points) = self.history.points.as_ref() else {
            return;
        };
        let Some(index) = points.index_at(hover.at) else {
            return;
        };
        let point = (points.ts[index], points.span[index]);
        let list = self
            .history
            .lanes
            .iter()
            .find(|l| l.id == hover.lane)
            .map_or(TipList::Nothing, |lane| self.history_tip_list(lane));
        if self.history.tip_for == Some(point) && self.history.tip_kind == list {
            return;
        }
        self.history.tip_for = Some(point);
        self.history.tip_kind = list;
        if list == TipList::Nothing {
            return;
        }
        // The programs' processes are counted in each frame: a bucket shows
        // those of a frame inside it.
        let frame_of = point.1 <= FRAME || list == TipList::Programs;
        let frame = points.frame[index];
        if self.options.fixture.is_some() {
            // A fixture holds one frame and one range table.
            let answer = if frame_of {
                self.data.get("historyFrame")
            } else {
                self.data.get("historyTable")
            };
            self.history.tip = answer.map(|answer| (point, answer.clone()));
            return;
        }
        if frame_of {
            let ts = if frame > 0 { frame } else { point.0 };
            self.request(
                "history_frame",
                json!({"ts": ts, "tip": [point.0, point.1]}),
                Some("historyTip"),
                window,
                cx,
            );
        } else {
            let lane = self
                .history
                .lanes
                .iter()
                .find(|l| l.id == hover.lane)
                .cloned();
            let trend = lane.map_or("cpu", |lane| self.history_trend_of(&lane));
            self.request(
                "history_table",
                json!({"from": point.0, "to": point.0 + point.1, "trend": trend, "points": 1,
                    "tip": [point.0, point.1]}),
                Some("historyTip"),
                window,
                cx,
            );
        }
    }

    /// The card by the pointer: when, the lane's readings then, or why nothing
    /// was recorded.
    pub(super) fn history_tip(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let p = self.palette;
        let hover = self.history.hover?;
        if self.history.drag.is_some() {
            return None;
        }
        let points = self.history.points.as_ref()?;
        let lane = self.history.lanes.iter().find(|l| l.id == hover.lane)?;
        let mut rows: Vec<AnyElement> = Vec::new();
        let head = |time: String, kind: Option<String>| {
            div()
                .flex()
                .items_baseline()
                .gap_2()
                .child(div().font_weight(theme::LABEL_WEIGHT).child(time))
                .when_some(kind, |d, kind| {
                    d.child(
                        div()
                            .text_size(theme::font(11.))
                            .text_color(p.faint)
                            .child(kind),
                    )
                })
                .into_any_element()
        };
        let bits = self.history_bits();
        match points.index_at(hover.at) {
            None => {
                let gap = points.gap_at(hover.at);
                let offsets = self.history_offsets_differ();
                rows.push(head(
                    with_offset(moment_label(hover.at, false), hover.at, offsets),
                    None,
                ));
                rows.push(
                    div()
                        .text_color(p.muted)
                        .child(match gap {
                            Some(gap) => tr!("history.gap.tip", reason = label(gap.reason.key())),
                            None => label("history.noRecordAt"),
                        })
                        .into_any_element(),
                );
            }
            Some(index) => {
                let start = points.ts[index];
                let length = points.span[index];
                let (time, kind) = if length <= FRAME {
                    (
                        moment_label(start, true),
                        Some(label(if length < FRAME {
                            "history.tip.second"
                        } else {
                            "history.tip.frame"
                        })),
                    )
                } else {
                    (
                        range_label(start, start + length, length < MINUTE),
                        Some(label("history.tip.average")),
                    )
                };
                rows.push(head(
                    with_offset(time, start, self.history_offsets_differ()),
                    kind,
                ));
                let mut readings = vec![self.history_reading(&lane.reading)];
                readings.push(
                    lane.overlay
                        .as_deref()
                        .and_then(|id| self.history_reading(id)),
                );
                for reading in readings.into_iter().flatten() {
                    let value = reading
                        .value(points, index)
                        .map(|v| reading.format(v, bits))
                        .unwrap_or_else(|| label("format.missing"));
                    rows.push(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .size(px(8.))
                                    .flex_shrink_0()
                                    .rounded(px(2.))
                                    .bg(reading.color),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_color(p.muted)
                                    .child(reading.long.clone()),
                            )
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .font_weight(theme::LABEL_WEIGHT)
                                    .font_features(theme::tabular())
                                    .child(value),
                            )
                            .into_any_element(),
                    );
                    if let (Some((a, b)), Some((na, nb))) =
                        (reading.part_values(points, index), &reading.parts)
                    {
                        rows.push(
                            div()
                                .pl(px(16.))
                                .truncate()
                                .text_size(theme::font(11.))
                                .text_color(p.faint)
                                .child(format!(
                                    "{na} {} · {nb} {}",
                                    reading.format(a, bits),
                                    reading.format(b, bits)
                                ))
                                .into_any_element(),
                        );
                    }
                    if length > FRAME
                        && let Some(peak) = reading.peak_value(points, index)
                    {
                        rows.push(
                            div()
                                .pl(px(16.))
                                .truncate()
                                .text_size(theme::font(11.))
                                .text_color(p.faint)
                                .child(tr!(
                                    "history.metric.peak",
                                    value = reading.format(peak, bits)
                                ))
                                .into_any_element(),
                        );
                    }
                }
                // Under the readings, what made them: the processes busiest by
                // the lane's reading (the frame's, or the bucket's averages), or
                // for the process count the programs with the most processes.
                // The rows stay while fewer are known, and a long name ends in
                // an ellipsis. Readings no process adds to list nothing.
                let live = values(&self.snapshot["processes"]);
                let icon_of = |name: &str| {
                    live.iter()
                        .find(|process| text(process, "name").eq_ignore_ascii_case(name))
                };
                let list = || {
                    div()
                        .mt(px(3.))
                        .pt(px(6.))
                        .border_t_1()
                        .border_color(p.stroke)
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                };
                let entry = |name: &str, value: String| {
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(self.process_icon(icon_of(name), 16., cx))
                        .child(div().flex_1().min_w_0().truncate().child(name.to_owned()))
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_color(p.muted)
                                .font_features(theme::tabular())
                                .child(value),
                        )
                };
                let blank = || {
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(div().size(px(16.)).flex_shrink_0())
                        .child("\u{a0}")
                };
                match self.history_tip_list(lane) {
                    TipList::Processes(key) => {
                        let ranked = self.history_tip_ranking(key);
                        let mut column = list();
                        for slot in 0..TIP_PROCESSES {
                            column = column.child(match ranked.get(slot) {
                                Some((name, value)) => {
                                    entry(name, table::format_process(key, *value, bits))
                                }
                                None => blank(),
                            });
                        }
                        rows.push(column.into_any_element());
                    }
                    TipList::Programs => {
                        // The latest frame answered (`history_ask_tip`): kept
                        // while the one for this point is on its way, as the
                        // processes' ranking is, so the rows do not empty and
                        // fill again with every step of the pointer.
                        let answer = self
                            .history
                            .tip
                            .as_ref()
                            .map(|(_, answer)| answer)
                            .filter(|answer| answer["system"].is_object());
                        let programs: Vec<(String, u64)> = answer
                            .map(|answer| {
                                values(&answer["programs"])
                                    .iter()
                                    .filter_map(|program| {
                                        Some((
                                            text(program, "name").to_owned(),
                                            program["count"].as_u64()?,
                                        ))
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();
                        // A frame recorded without the counts lists nothing.
                        if answer.is_none() || !programs.is_empty() {
                            let shown = programs.len().min(TIP_PROCESSES);
                            let mut column = list();
                            for slot in 0..TIP_PROCESSES {
                                column = column.child(match programs.get(slot) {
                                    Some((name, count)) if slot < shown => {
                                        entry(name, crate::regional::count(*count as f64))
                                    }
                                    _ => blank(),
                                });
                            }
                            // The other programs, and the processes they have:
                            // a row of its own, kept while not known, so the
                            // tooltip keeps its height.
                            let rest = answer.and_then(|answer| {
                                let distinct = answer["programCount"].as_u64().unwrap_or(0);
                                let total = answer["system"]["processes"].as_f64().unwrap_or(0.);
                                let listed: u64 = programs[..shown].iter().map(|(_, n)| n).sum();
                                (distinct > shown as u64).then(|| {
                                    (distinct - shown as u64, (total - listed as f64).max(0.))
                                })
                            });
                            column = column.child(match rest {
                                Some((others, processes)) => div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .text_color(p.muted)
                                    .child(div().size(px(16.)).flex_shrink_0())
                                    .child(div().flex_1().min_w_0().truncate().child(
                                        theme::counted("history.tip.otherPrograms", others, &[]),
                                    ))
                                    .child(
                                        div()
                                            .flex_shrink_0()
                                            .font_features(theme::tabular())
                                            .child(crate::regional::count(processes)),
                                    ),
                                None => blank(),
                            });
                            rows.push(column.into_any_element());
                        }
                    }
                    TipList::Nothing => {}
                }
            }
        }
        rows.push(
            div()
                .mt(px(3.))
                .pt(px(6.))
                .border_t_1()
                .border_color(p.stroke)
                .text_size(theme::font(11.))
                .text_color(p.faint)
                .child(label("history.tip.hint"))
                .into_any_element(),
        );
        // Beside the pointer, on its left where the right has no room: at a
        // fixed width, it turns over at one place and keeps its size.
        let flip = hover.position.x + px(TIP_GAP + TIP_WIDTH)
            > window.viewport_size().width - px(TIP_MARGIN);
        Some(
            deferred(
                anchored()
                    .anchor(if flip {
                        gpui_kit::gpui::Anchor::TopRight
                    } else {
                        gpui_kit::gpui::Anchor::TopLeft
                    })
                    .position(point(
                        if flip {
                            hover.position.x - px(TIP_GAP)
                        } else {
                            hover.position.x + px(TIP_GAP)
                        },
                        hover.position.y + px(16.),
                    ))
                    .snap_to_window_with_margin(px(TIP_MARGIN))
                    .child(
                        div()
                            .w(px(TIP_WIDTH))
                            .flex()
                            .flex_col()
                            .gap(px(3.))
                            .px(px(10.))
                            .py(px(8.))
                            .rounded(px(6.))
                            .border_1()
                            .border_color(p.stroke_strong)
                            .bg(widgets::solid(p.tip))
                            .text_size(theme::font(12.))
                            .line_height(relative(1.4))
                            .children(rows),
                    ),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }
}
