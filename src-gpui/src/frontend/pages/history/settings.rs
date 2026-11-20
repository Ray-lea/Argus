//! The history's cards on the Settings page, under data and history: whether
//! it records, what it records and how many processes a frame keeps, how long
//! and how much it keeps, how older days are merged, where the records are
//! kept, and what they take now.
use super::super::settings::short_time;
use super::*;
use crate::frontend::widgets::{ButtonExt, Tone};

/// The time limits offered, in days, and the size limits, in MiB; 0 is none.
const KEEP_DAYS: [u64; 7] = [7, 14, 30, 90, 180, 365, 0];
const KEEP_MIB: [u64; 7] = [256, 512, 1024, 2048, 5120, 10240, 0];
const MIB: u64 = 1 << 20;
/// How older days are kept, and after how many days they are merged.
const DETAILS: [(&str, Option<u64>); 3] =
    [("full", None), ("minute7", Some(7)), ("minute30", Some(30))];
/// Processes a frame keeps: about 20, about 40, every active one.
const PROCESSES: [u64; 3] = [20, 40, 0];
/// A merged day beside its five-second records: 12.8 % in a day's replay.
const MERGED: f64 = 0.13;
/// What can be left out, in the order shown, and whether it is read through
/// the privileged service when Argus runs with standard rights.
const GROUPS: [(&str, bool); 11] = [
    ("cpuExtra", false),
    ("sensors", true),
    ("memoryDetail", false),
    ("counts", false),
    ("disks", false),
    ("nics", false),
    ("gpus", false),
    ("batteryDetail", false),
    ("processDisk", false),
    ("processNetwork", true),
    ("processGpu", false),
];

/// A limit as saved: a number, or none (null) for no limit; missing is the default.
fn limit(settings: &Value, key: &str, default: u64) -> Option<u64> {
    match settings.get(key) {
        None => Some(default),
        Some(value) => value.as_u64(),
    }
}

/// What `days` of records take at `per_day` bytes a day, merged after `merge` days.
fn kept_bytes(days: u64, per_day: f64, merge: Option<u64>) -> f64 {
    match merge {
        None => days as f64 * per_day,
        Some(merge) => {
            days.min(merge) as f64 * per_day + days.saturating_sub(merge) as f64 * per_day * MERGED
        }
    }
}

/// How many whole days fit in `bytes` (at most ten years).
fn days_in(bytes: u64, per_day: f64, merge: Option<u64>) -> u64 {
    let mut days = 0;
    while days < 3650 && kept_bytes(days + 1, per_day, merge) <= bytes as f64 {
        days += 1;
    }
    days
}

impl Desktop {
    /// The history's cards, in the order the settings list them.
    pub(in crate::frontend) fn history_setting_cards(
        &mut self,
        settings: &Value,
        cards: &mut Vec<AnyElement>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let usage = self.data.get("historyUsage").cloned();
        cards.push(self.history_recording_card(settings, window, cx));
        cards.push(self.history_record_card(settings, usage.as_ref(), cx));
        cards.push(self.history_limit_card(settings, usage.as_ref(), cx));
        cards.push(self.history_detail_card(settings, cx));
        cards.push(self.history_place_card(usage.as_ref(), cx));
        cards.push(self.history_saved_card(usage.as_ref(), cx));
    }

    /// Whether readings that need the privileged service are read: Argus runs
    /// as administrator, or the service runs and lets it in.
    fn history_service_ready(&self) -> bool {
        boolean(&self.data["info"], "elevated")
            || self.data.get("privilege").is_some_and(|s| {
                text(s, "state") == "running" && boolean(s, "authorized") && s["error"].is_null()
            })
    }

    /// Saves a part of the history's options; the rest stay as they are.
    fn history_set(&mut self, options: Value, window: &mut Window, cx: &mut Context<Self>) {
        self.request(
            "set_history_options",
            json!({ "options": options }),
            Some("settings"),
            window,
            cx,
        );
    }

    fn history_recording_card(
        &self,
        settings: &Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let history = boolean(settings, "history");
        let toggle = self.setting_toggle(
            "history",
            history,
            "set_history_enabled",
            "enabled",
            window,
            cx,
        );
        self.setting_text(
            "history",
            "HistoryIcon",
            "settings.data.history.title",
            if history {
                "settings.data.history.on"
            } else {
                "settings.data.history.off"
            },
            vec![toggle],
        )
    }

    /// What is recorded: the groups that can be left out, each with what it
    /// holds, the processes a frame keeps, and what a day takes.
    fn history_record_card(
        &self,
        settings: &Value,
        usage: Option<&Value>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.palette;
        let record = &settings["historyRecord"];
        let ready = self.history_service_ready();
        let option = |key: &'static str, on: bool, fixed: bool, badge: Option<AnyElement>| {
            div()
                .id(SharedString::from(format!("history-record-{key}")))
                .flex()
                .items_start()
                .gap(px(8.))
                .min_w_0()
                .when(!fixed, |d| d.cursor_pointer())
                .child(div().pt(px(1.)).child(self.check_box(on, fixed)))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .min_w_0()
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .items_center()
                                .gap(px(6.))
                                .child(label(&format!("settings.data.history.record.{key}")))
                                .children(badge),
                        )
                        .child(
                            div()
                                .text_size(theme::font(12.))
                                .text_color(p.muted)
                                .child(label(&format!("settings.data.history.record.{key}Desc"))),
                        ),
                )
                .when(!fixed, |d| {
                    d.on_click(cx.listener(move |this, _, window, cx| {
                        let options = json!({ "record": { key: !on } });
                        this.history_set(options, window, cx);
                    }))
                })
                .into_any_element()
        };
        let mut grid = div()
            .grid()
            .grid_cols(2)
            .gap_x(px(24.))
            .gap_y(px(10.))
            .child(option(
                "always",
                true,
                true,
                Some(self.history_badge(None, label("settings.data.history.record.alwaysBadge"))),
            ));
        for (key, service) in GROUPS {
            let on = record.get(key).and_then(Value::as_bool).unwrap_or(true);
            let badge = service.then(|| {
                if ready {
                    self.history_badge(
                        Some(("ShieldCheckIcon", p.muted)),
                        label("history.menu.byService"),
                    )
                } else {
                    self.history_badge(
                        Some(("ShieldAlertIcon", p.io)),
                        label("history.menu.needsService"),
                    )
                }
            });
            grid = grid.child(option(key, on, false, badge));
        }
        let processes = settings["historyProcesses"].as_u64().unwrap_or(20);
        let select = self.select(
            "history-processes",
            label("settings.data.history.record.processes"),
            PROCESSES
                .iter()
                .map(|n| label(&format!("settings.data.history.record.processes{n}")))
                .collect(),
            PROCESSES.iter().position(|n| *n == processes),
            false,
            move |this, index, window, cx| {
                this.history_set(json!({ "processes": PROCESSES[index] }), window, cx);
            },
            cx,
        );
        let per_day = usage
            .map(|u| u["estimate"]["bytesPerDay"].as_f64().unwrap_or(0.))
            .unwrap_or(0.);
        let body = div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(grid)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .child(label("settings.data.history.record.processes"))
                            .child(
                                div()
                                    .text_size(theme::font(12.))
                                    .text_color(p.muted)
                                    .child(label("settings.data.history.record.processesDesc")),
                            ),
                    )
                    .child(select),
            )
            .when(per_day > 0., |d| {
                d.child(
                    div()
                        .text_size(theme::font(12.))
                        .text_color(p.muted)
                        .child(tr!(
                            "settings.data.history.record.estimate",
                            size = theme::bytes(per_day, false)
                        )),
                )
            })
            .into_any_element();
        self.setting_with_body(
            "historyRecord",
            "LayersIcon",
            "settings.data.history.record.title",
            "settings.data.history.record.desc",
            body,
        )
    }

    /// A quiet framed note beside an option: always recorded, or the service.
    fn history_badge(&self, glyph: Option<(&str, Hsla)>, text: String) -> AnyElement {
        let p = self.palette;
        div()
            .flex()
            .items_center()
            .gap(px(4.))
            .h(px(18.))
            .px(px(5.))
            .border_1()
            .border_color(p.stroke_strong)
            .rounded(px(4.))
            .text_size(theme::font(11.))
            .text_color(p.muted)
            .whitespace_nowrap()
            .when_some(glyph, |d, (glyph, color)| {
                d.child(icon(glyph, color).size(px(12.)))
            })
            .child(text)
            .into_any_element()
    }

    /// How long and how much is kept: either limit or none, the first reached
    /// applying; what the two give with what a day takes now.
    fn history_limit_card(
        &self,
        settings: &Value,
        usage: Option<&Value>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.palette;
        let days = limit(settings, "historyKeepDays", 30).filter(|d| *d > 0);
        let bytes = limit(settings, "historyKeepBytes", 2048 * MIB).filter(|b| *b > 0);
        let merge = DETAILS
            .iter()
            .find(|(key, _)| *key == text(settings, "historyDetail"))
            .map_or(Some(7), |(_, merge)| *merge);
        let time = self.select(
            "history-keep-days",
            label("settings.data.history.limit.time"),
            KEEP_DAYS
                .iter()
                .map(|d| match d {
                    0 => label("settings.data.history.limit.none"),
                    365 => label("settings.data.history.limit.year"),
                    d => tr!("settings.data.history.limit.days", count = *d),
                })
                .collect(),
            KEEP_DAYS.iter().position(|d| Some(*d) == days.or(Some(0))),
            false,
            move |this, index, window, cx| {
                let value = KEEP_DAYS[index];
                let narrower = value != 0 && days.is_none_or(|d| value < d);
                let options = json!({ "keepDays": (value != 0).then_some(value) });
                this.history_confirm_limit(narrower, options, window, cx);
            },
            cx,
        );
        let size = self.select(
            "history-keep-bytes",
            label("settings.data.history.limit.space"),
            KEEP_MIB
                .iter()
                .map(|m| match m {
                    0 => label("settings.data.history.limit.none"),
                    m => theme::bytes((m * MIB) as f64, false),
                })
                .collect(),
            KEEP_MIB
                .iter()
                .position(|m| Some(m * MIB) == bytes.or(Some(0))),
            false,
            move |this, index, window, cx| {
                let value = KEEP_MIB[index] * MIB;
                let narrower = value != 0 && bytes.is_none_or(|b| value < b);
                let options = json!({ "keepBytes": (value != 0).then_some(value) });
                this.history_confirm_limit(narrower, options, window, cx);
            },
            cx,
        );
        let per_day = usage
            .map(|u| u["estimate"]["bytesPerDay"].as_f64().unwrap_or(0.))
            .unwrap_or(0.);
        let desc = match (days, bytes) {
            (None, None) => div()
                .text_color(p.danger)
                .child(label("settings.data.history.limit.unbounded")),
            _ if per_day <= 0. => div().child(label("settings.data.history.limit.desc")),
            _ => {
                let by_size = bytes.map(|b| days_in(b, per_day, merge));
                let kept = match (days, by_size) {
                    (Some(d), Some(s)) => d.min(s),
                    (Some(d), None) => d,
                    (None, Some(s)) => s,
                    (None, None) => 0,
                };
                let by = if by_size.is_some_and(|s| days.is_none_or(|d| s < d)) {
                    "settings.data.history.limit.bySpace"
                } else {
                    "settings.data.history.limit.byTime"
                };
                div()
                    .flex()
                    .flex_col()
                    .child(label("settings.data.history.limit.desc"))
                    .child(tr!(
                        "settings.data.history.limit.estimate",
                        count = kept,
                        by = label(by),
                        size = theme::bytes(kept_bytes(kept, per_day, merge), false)
                    ))
            }
        };
        let caption = |key: &str| {
            div()
                .text_size(theme::font(12.))
                .text_color(p.muted)
                .child(label(key))
                .into_any_element()
        };
        self.setting(
            "historyLimit",
            self.glyph("CalendarRangeIcon"),
            label("settings.data.history.limit.title"),
            Some(desc.into_any_element()),
            None,
            vec![
                caption("settings.data.history.limit.time"),
                time,
                caption("settings.data.history.limit.space"),
                size,
            ],
        )
    }

    /// Saves a limit, asking first when it is narrower: what no longer fits goes.
    fn history_confirm_limit(
        &mut self,
        narrower: bool,
        options: Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !narrower {
            self.history_set(options, window, cx);
            return;
        }
        self.ask(
            label("settings.data.history.limit.confirmTitle"),
            label("settings.data.history.limit.confirmBody"),
            label("settings.data.history.limit.apply"),
            true,
            move |this, window, cx| this.history_set(options.clone(), window, cx),
            window,
            cx,
        );
    }

    /// How days past a week or a month are kept: every five-second frame, or
    /// one frame a minute.
    fn history_detail_card(&self, settings: &Value, cx: &mut Context<Self>) -> AnyElement {
        let current = match text(settings, "historyDetail") {
            "" => "minute7",
            detail => detail,
        };
        let merge_now = DETAILS
            .iter()
            .find(|(key, _)| *key == current)
            .and_then(|(_, merge)| *merge);
        let select = self.select(
            "history-detail",
            label("settings.data.history.detail.title"),
            DETAILS
                .iter()
                .map(|(key, _)| label(&format!("settings.data.history.detail.{key}")))
                .collect(),
            DETAILS.iter().position(|(key, _)| *key == current),
            false,
            move |this, index, window, cx| {
                let (key, merge) = DETAILS[index];
                let options = json!({ "detail": key });
                // Merging sooner than now loses detail that cannot come back.
                let sooner = merge.is_some_and(|m| merge_now.is_none_or(|now| m < now));
                if !sooner {
                    this.history_set(options, window, cx);
                    return;
                }
                this.ask(
                    label("settings.data.history.detail.confirmTitle"),
                    tr!(
                        "settings.data.history.detail.confirmBody",
                        count = merge.unwrap_or(7)
                    ),
                    label("settings.data.history.detail.merge"),
                    true,
                    move |this, window, cx| this.history_set(options.clone(), window, cx),
                    window,
                    cx,
                );
            },
            cx,
        );
        self.setting_text(
            "historyDetail",
            "ChartLineIcon",
            "settings.data.history.detail.title",
            if current == "full" {
                "settings.data.history.detail.fullDesc"
            } else {
                "settings.data.history.detail.mergedDesc"
            },
            vec![select],
        )
    }

    /// Where the records are kept: the folder (the default one or another),
    /// opened in File Explorer or changed; a folder chosen asks, under the
    /// card, whether the records go along.
    fn history_place_card(&self, usage: Option<&Value>, cx: &mut Context<Self>) -> AnyElement {
        let p = self.palette;
        let dir = usage.map(|u| text(u, "dir").to_owned()).unwrap_or_default();
        let custom = usage.is_some_and(|u| boolean(u, "custom"));
        let busy = self.ui.busy.contains("historyMove");
        let desc = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(8.))
            .child(div().min_w_0().text_color(p.ink).child(dir))
            .when(!custom && usage.is_some(), |d| {
                d.child(
                    div()
                        .text_color(p.faint)
                        .child(label("settings.data.history.place.default")),
                )
            })
            .into_any_element();
        let open = self
            .btn(
                "history-open-dir",
                label("settings.data.history.place.open"),
                Some("FolderOpenIcon"),
                Tone::Plain,
            )
            .inert(busy)
            .on_click(cx.listener(|this, _, window, cx| {
                this.request("history_open_dir", json!({}), None, window, cx);
            }))
            .into_any_element();
        let change = self
            .btn(
                "history-change-dir",
                label("settings.data.history.place.change"),
                None,
                Tone::Plain,
            )
            .inert(busy)
            .on_click(cx.listener(|this, _, window, cx| {
                this.ui.busy.insert("pickHistory".into());
                this.request("pick_path", json!({ "folder": true }), None, window, cx);
            }))
            .into_any_element();
        let mut controls = vec![open, change];
        if custom {
            controls.push(
                self.btn(
                    "history-default-dir",
                    label("settings.data.history.place.reset"),
                    None,
                    Tone::Plain,
                )
                .inert(busy)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.data.insert("historyMoveTo".into(), json!(""));
                    this.data.insert("historyMoveRecords".into(), json!(true));
                    cx.notify();
                }))
                .into_any_element(),
            );
        }
        let body = self
            .data
            .get("historyMoveTo")
            .and_then(Value::as_str)
            .map(|to| {
                let to = if to.is_empty() {
                    usage
                        .map(|u| text(u, "defaultDir").to_owned())
                        .unwrap_or_default()
                } else {
                    to.to_owned()
                };
                self.history_move_panel(to, usage, busy, cx)
            });
        self.setting_card(
            "historyPlace",
            self.glyph("FolderIcon"),
            label("settings.data.history.place.title"),
            Some(desc),
            None,
            controls,
            body,
        )
    }

    /// The folder chosen, whether the records go along, and the step itself.
    fn history_move_panel(
        &self,
        to: String,
        usage: Option<&Value>,
        busy: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.palette;
        let along = self
            .data
            .get("historyMoveRecords")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let size = theme::bytes(usage.map(|u| number(u, "bytes")).unwrap_or(0.), false);
        let cancel = self
            .btn(
                "history-move-cancel",
                label("common.cancel"),
                None,
                Tone::Plain,
            )
            .inert(busy)
            .on_click(cx.listener(|this, _, _, cx| {
                this.data.remove("historyMoveTo");
                cx.notify();
            }))
            .into_any_element();
        let confirm = self
            .btn(
                "history-move-confirm",
                label(if along {
                    "settings.data.history.place.moveAndChange"
                } else {
                    "settings.data.history.place.changeOnly"
                }),
                None,
                Tone::Accent,
            )
            .inert(busy)
            .on_click(cx.listener(move |this, _, window, cx| {
                let Some(to) = this
                    .data
                    .get("historyMoveTo")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                else {
                    return;
                };
                let (command, args) = if to.is_empty() {
                    ("history_reset_dir", json!({ "moveRecords": along }))
                } else {
                    ("history_move", json!({ "dir": to, "moveRecords": along }))
                };
                this.ui.busy.insert("historyMove".into());
                this.request(command, args, Some("settings"), window, cx);
                cx.notify();
            }))
            .into_any_element();
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .p(px(12.))
            .border_1()
            .border_color(p.stroke)
            .rounded(px(6.))
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_color(p.muted)
                            .child(label("settings.data.history.place.new")),
                    )
                    .child(div().min_w_0().child(to)),
            )
            .child(
                div()
                    .id("history-move-records")
                    .flex()
                    .items_start()
                    .gap(px(8.))
                    .cursor_pointer()
                    .child(div().pt(px(1.)).child(self.check_box(along, false)))
                    .child(tr!("settings.data.history.place.records", size = size))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.data.insert("historyMoveRecords".into(), json!(!along));
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .text_size(theme::font(12.))
                    .text_color(p.muted)
                    .child(label("settings.data.history.place.pause")),
            )
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(8.))
                    .child(cancel)
                    .child(confirm),
            )
            .into_any_element()
    }

    /// What is kept now: its size and first day, split into the system's
    /// readings, the processes and the observations; and a way to clear it.
    fn history_saved_card(&self, usage: Option<&Value>, cx: &mut Context<Self>) -> AnyElement {
        let p = self.palette;
        let bytes = usage.map(|u| number(u, "bytes")).unwrap_or(0.);
        let summary = match usage {
            None => label("settings.data.history.counting"),
            Some(_) if bytes <= 0. => label("settings.data.history.empty"),
            Some(usage) => {
                let size = theme::bytes(bytes, false);
                match usage["first"].as_i64().filter(|t| *t > 0) {
                    Some(first) => tr!(
                        "settings.data.history.sizeSince",
                        size = size,
                        date = short_time(first)
                    ),
                    None => tr!("settings.data.history.size", size = size),
                }
            }
        };
        let parts = [
            ("system", p.accent),
            ("processes", p.memory),
            ("watch", p.io),
        ]
        .map(|(key, color)| {
            let size = usage.map_or(0., |u| number(&u["breakdown"], key));
            (key, color, size)
        });
        let total = parts.iter().map(|(_, _, size)| size).sum::<f64>();
        let desc = div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(summary)
            .when(total > 0., |d| {
                d.child(
                    div()
                        .flex()
                        .h(px(6.))
                        .w_full()
                        .max_w(px(360.))
                        .rounded(px(3.))
                        .overflow_hidden()
                        .bg(p.stroke)
                        .children(parts.iter().filter(|(_, _, s)| *s > 0.).map(
                            |(_, color, size)| {
                                div().h_full().w(relative((size / total) as f32)).bg(*color)
                            },
                        )),
                )
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(12.))
                        .children(parts.iter().map(|(key, color, size)| {
                            div()
                                .flex()
                                .items_center()
                                .gap(px(5.))
                                .child(div().size(px(8.)).rounded(px(2.)).bg(*color))
                                .child(tr!(
                                    "settings.data.history.part",
                                    name = label(&format!("settings.data.history.parts.{key}")),
                                    size = theme::bytes(*size, false)
                                ))
                        })),
                )
            })
            .into_any_element();
        let clearing = self.ui.busy.contains("clearHistory");
        let clear = self
            .btn(
                "clear-history",
                label("settings.data.history.clear"),
                Some("TrashIcon"),
                Tone::Plain,
            )
            .inert(clearing || bytes <= 0.)
            .on_click(cx.listener(|this, _, window, cx| this.clear_history(window, cx)))
            .into_any_element();
        self.setting(
            "savedHistory",
            self.glyph("StorageIcon"),
            label("settings.data.history.saved"),
            Some(desc),
            None,
            vec![clear],
        )
    }
}
