//! Leak alerts on the process page. The sampler reports processes that appear
//! to leak handles or processes (`argus_core::leaks`); here the interface
//! picks the findings to show, marks the processes they concern, lists them
//! under the header's mark and explains each in the details pane.
use super::layout::Side;
use super::*;
use std::borrow::Borrow;
use std::collections::BTreeSet;
use widgets::{Handler, ToastKind, Tone};

/// Whether leaks are pointed out; on by default.
pub(super) const ALERTS: &str = "leakAlerts";
/// Programs whose findings are not shown: lowercase paths, or lowercase
/// names where the path is unknown.
pub(super) const IGNORED: &str = "leakIgnored";
/// Where the header's mark is recorded, for its menu.
pub(super) const MARK: &str = "leak-mark";
/// Kinds in the order a row with several shows them.
const KINDS: [&str; 3] = ["handles", "children", "orphans"];
const MAX_IGNORED: usize = 256;

/// Which findings are shown.
#[derive(Clone, Debug, Default)]
pub(super) struct Filter {
    pub enabled: bool,
    pub ignored: BTreeSet<String>,
}

/// The key a finding is ignored by: its program.
pub(super) fn program(leak: &Value) -> String {
    leak["path"]
        .as_str()
        .filter(|path| !path.is_empty())
        .unwrap_or(text(leak, "name"))
        .to_lowercase()
}

fn kind(leak: &Value) -> Option<&'static str> {
    KINDS.into_iter().find(|kind| text(leak, "kind") == *kind)
}

fn instance(value: &Value) -> Option<(u64, i64)> {
    Some((value["pid"].as_u64()?, value["createTime"].as_i64()?))
}

/// Writes the findings to show as `leakAlerts` and marks each process they
/// concern with `leak`, its kind: the leaking process, the parent whose
/// children pile up, every leftover instance. It starts again from the
/// sampler's `leaks` each time, so a changed filter applies to the same sample.
pub(super) fn annotate(model: &mut Value, filter: &Filter) {
    if !model.is_object() {
        return;
    }
    let shown: Vec<Value> = if filter.enabled {
        values(&model["leaks"])
            .iter()
            .filter(|leak| kind(leak).is_some() && !filter.ignored.contains(&program(leak)))
            .cloned()
            .collect()
    } else {
        Vec::new()
    };
    let mut marks: HashMap<(u64, i64), &'static str> = HashMap::new();
    for kind in KINDS {
        for leak in shown.iter().filter(|leak| text(leak, "kind") == kind) {
            let concerned: Vec<&Value> = if kind == "orphans" {
                values(&leak["members"]).iter().collect()
            } else {
                vec![leak]
            };
            for process in concerned.into_iter().filter_map(instance) {
                marks.entry(process).or_insert(kind);
            }
        }
    }
    if let Some(processes) = model["processes"].as_array_mut() {
        for process in processes {
            let mark = instance(process).and_then(|key| marks.get(&key).copied());
            if let Some(object) = process.as_object_mut() {
                match mark {
                    Some(kind) => {
                        object.insert("leak".into(), json!(kind));
                    }
                    None => {
                        object.remove("leak");
                    }
                }
            }
        }
    }
    model["leakAlerts"] = Value::Array(shown);
}

/// The kind a row shows: the first of its processes' marks, in `KINDS` order.
pub(super) fn row_kind<V: Borrow<Value>>(members: &[V]) -> Option<&'static str> {
    KINDS.into_iter().find(|kind| {
        members
            .iter()
            .any(|member| text(member.borrow(), "leak") == *kind)
    })
}

/// The tag a marked row carries.
pub(super) fn tag_label(kind: &str) -> String {
    label(&format!("processes.leak.kind.{kind}"))
}

fn kind_icon(kind: &str) -> &'static str {
    match kind {
        "handles" => "HandleLeakIcon",
        "children" => "ChildLeakIcon",
        _ => "OrphanIcon",
    }
}

fn minutes(leak: &Value) -> String {
    let minutes = (number(leak, "spanMs") / 60_000.).round().max(1.);
    theme::quantity("performance.span.minutes", minutes)
}

/// What the finding rests on, in one sentence.
pub(super) fn describe(leak: &Value) -> String {
    let count = theme::count(number(leak, "count"));
    let growth = theme::count(number(leak, "growth"));
    let span = minutes(leak);
    match (text(leak, "kind"), text(leak, "evidence")) {
        ("handles", "extreme") if number(leak, "growth") > 0. => tr!(
            "processes.leak.handlesExtremeGrowing",
            handles = count,
            growth = growth,
            span = span
        ),
        ("handles", "extreme") => tr!("processes.leak.handlesExtreme", handles = count),
        ("handles", _) => tr!(
            "processes.leak.handlesGrowing",
            handles = count,
            growth = growth,
            span = span
        ),
        ("children", _) => tr!(
            "processes.leak.childrenGrowing",
            children = count,
            growth = growth,
            span = span
        ),
        _ => tr!(
            "processes.leak.orphansIdle",
            instances = count,
            name = text(leak, "name"),
            span = span
        ),
    }
}

/// The figure a menu entry ends with.
fn brief(leak: &Value) -> String {
    let count = theme::count(number(leak, "count"));
    match text(leak, "kind") {
        "handles" => tr!("processes.leak.brief.handles", handles = count),
        "children" => tr!("processes.leak.brief.children", children = count),
        _ => tr!("processes.leak.brief.orphans", instances = count),
    }
}

impl Desktop {
    pub(super) fn leak_filter(&self) -> Filter {
        Filter {
            enabled: self.prefs.get(ALERTS, true),
            ignored: self.prefs.get(IGNORED, BTreeSet::new()),
        }
    }

    /// The findings shown with the sample on screen.
    pub(super) fn leak_alerts(&self) -> &[Value] {
        values(&self.snapshot["leakAlerts"])
    }

    /// The finding that marked `process`.
    pub(super) fn leak_of(&self, process: &Value) -> Option<&Value> {
        let kind = process["leak"].as_str()?;
        let key = instance(process)?;
        self.leak_alerts().iter().find(|leak| {
            text(leak, "kind") == kind
                && if kind == "orphans" {
                    values(&leak["members"])
                        .iter()
                        .any(|member| instance(member) == Some(key))
                } else {
                    instance(leak) == Some(key)
                }
        })
    }

    /// The first finding among a row's processes, as `row_kind` picks it.
    pub(super) fn row_leak<V: Borrow<Value>>(&self, members: &[V]) -> Option<&Value> {
        let kind = row_kind(members)?;
        members
            .iter()
            .map(Borrow::borrow)
            .filter(|member| text(member, "leak") == kind)
            .find_map(|member| self.leak_of(member))
    }

    /// The name a finding goes by: its process's, as the list shows it.
    fn leak_title(&self, leak: &Value) -> String {
        let key = instance(leak);
        values(&self.snapshot["processes"])
            .iter()
            .find(|process| instance(process) == key)
            .map(model::title)
            .filter(|title| !title.is_empty())
            .unwrap_or_else(|| text(leak, "name").to_owned())
    }

    /// Applies the filter to the sample on screen at once.
    fn refresh_leaks(&mut self, cx: &mut Context<Self>) {
        let filter = self.leak_filter();
        annotate(&mut self.snapshot, &filter);
        if self.page == "processes" {
            self.rebuild_rows(false, cx);
        }
        cx.notify();
    }

    pub(super) fn set_leak_alerts(&mut self, on: bool, cx: &mut Context<Self>) {
        self.prefs.set(ALERTS, on);
        self.refresh_leaks(cx);
    }

    pub(super) fn set_ignored_leaks(&mut self, ignored: BTreeSet<String>, cx: &mut Context<Self>) {
        self.prefs.set(IGNORED, ignored);
        self.refresh_leaks(cx);
    }

    pub(super) fn restore_ignored_leaks(&mut self, cx: &mut Context<Self>) {
        self.set_ignored_leaks(BTreeSet::new(), cx);
    }

    /// Stops showing `leak`'s program, with a notice that can take it back.
    pub(super) fn ignore_leak(&mut self, leak: &Value, cx: &mut Context<Self>) {
        let program = program(leak);
        let mut ignored = self.leak_filter().ignored;
        if ignored.len() >= MAX_IGNORED {
            return;
        }
        ignored.insert(program.clone());
        let name = self.leak_title(leak);
        self.set_ignored_leaks(ignored, cx);
        let undo: Handler = Rc::new(move |this, _, cx| {
            let mut ignored = this.leak_filter().ignored;
            ignored.remove(&program);
            this.set_ignored_leaks(ignored, cx);
        });
        self.toast_with(
            ToastKind::Info,
            tr!("processes.leak.ignored", name = name),
            8000,
            vec![(label("processes.leak.undo"), undo)],
        );
    }

    /// The header mark's list: each finding, which shows its process when
    /// picked, then the way to the setting.
    pub(super) fn open_leak_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut items: Vec<MenuItem> = self
            .leak_alerts()
            .iter()
            .map(|leak| {
                MenuItem::new(
                    self.leak_title(leak),
                    Action::Preference(
                        "revealProcess".into(),
                        json!({"pid": leak["pid"], "createTime": leak["createTime"]}),
                    ),
                )
                .icon(kind_icon(text(leak, "kind")))
                .detail(brief(leak))
            })
            .collect();
        if items.is_empty() {
            return;
        }
        items.push(MenuItem::separator());
        items.push(
            MenuItem::new(
                label("processes.leak.settings"),
                Action::Preference("settingsSection".into(), json!("language")),
            )
            .icon("SettingsIcon"),
        );
        self.open_anchored(items, MARK, Side::Under, window, cx);
    }

    /// The details pane's notice for a process the alerts mark: what was seen,
    /// how the count went, what can be done, and the way to stop the alert.
    pub(super) fn leak_card<V: Borrow<Value>>(
        &self,
        members: &[V],
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let leak = self.row_leak(members)?.clone();
        let kind = kind(&leak)?;
        let p = self.palette;
        let trend: Vec<Option<f64>> = values(&leak["trend"])
            .iter()
            .map(|value| value.as_f64())
            .collect();
        let chart = (trend.iter().flatten().count() >= 2).then(|| {
            let top = trend.iter().flatten().copied().fold(0., f64::max).max(1.);
            let low = trend.iter().flatten().copied().fold(f64::MAX, f64::min);
            let last = trend.iter().flatten().last().copied().unwrap_or(top);
            let span = minutes(&leak);
            let caption = if kind == "handles" {
                tr!("processes.leak.trend.handles", span = span)
            } else {
                tr!("processes.leak.trend.children", span = span)
            };
            let color = p.io;
            div()
                .mt(px(10.))
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(
                    div()
                        .flex()
                        .justify_between()
                        .gap(px(8.))
                        .text_size(theme::font(11.))
                        .text_color(p.faint)
                        .child(caption)
                        .child(format!("{} → {}", theme::count(low), theme::count(last))),
                )
                .child(
                    div().h(px(32.)).child(
                        canvas(
                            |_, _, _| {},
                            move |bounds, _, window, _| {
                                let scale = window.scale_factor();
                                let size = (
                                    (f32::from(bounds.size.width) * scale).round() as u32,
                                    (f32::from(bounds.size.height) * scale).round() as u32,
                                );
                                if size.0 == 0 || size.1 == 0 {
                                    return;
                                }
                                // From the lowest reading, so the rise fills the box.
                                let rise: Vec<Option<f64>> =
                                    trend.iter().map(|v| v.map(|v| v - low)).collect();
                                if let Some(image) = charts::sparkline_image(
                                    &rise,
                                    (top - low).max(1.),
                                    size,
                                    scale,
                                    color,
                                    window,
                                ) {
                                    let _ = window.paint_image(
                                        bounds,
                                        bounds,
                                        Corners::default(),
                                        image,
                                        0,
                                        false,
                                    );
                                }
                            },
                        )
                        .size_full(),
                    ),
                )
        });
        let ignore = leak.clone();
        let button = self
            .btn_small(
                "leak-ignore",
                label("processes.leak.ignore"),
                None,
                Tone::Ghost,
            )
            .on_click(cx.listener(move |this, _, _, cx| this.ignore_leak(&ignore, cx)));
        Some(
            div()
                .id("leak-card")
                .mt(px(14.))
                .px(px(12.))
                .py(px(10.))
                .rounded(px(6.))
                .border_1()
                .border_color(p.io.opacity(0.35))
                .bg(p.io.opacity(0.07))
                .text_size(theme::font(12.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(icon("WarningIcon", p.io).size(px(14.)).flex_shrink_0())
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(theme::font(13.))
                                .font_weight(theme::LABEL_WEIGHT)
                                .text_color(p.ink)
                                .child(label(&format!("processes.leak.title.{kind}"))),
                        )
                        .child(button),
                )
                .child(div().mt(px(6.)).text_color(p.ink).child(describe(&leak)))
                .children(chart)
                .child(
                    div()
                        .mt(px(8.))
                        .text_color(p.muted)
                        .child(label(&format!("processes.leak.advice.{kind}"))),
                )
                .into_any_element(),
        )
    }

    /// The setting: whether to point leaks out, and the programs set aside.
    pub(super) fn leak_setting(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let filter = self.leak_filter();
        let mut controls = vec![self.toggle(
            "leakAlerts",
            label("settings.leaks.title"),
            filter.enabled,
            false,
            |this, on, _, cx| this.set_leak_alerts(on, cx),
            window,
            cx,
        )];
        if !filter.ignored.is_empty() {
            controls.insert(
                0,
                self.btn_small(
                    "leak-restore",
                    theme::counted("settings.leaks.restore", filter.ignored.len() as u64, &[]),
                    None,
                    Tone::Ghost,
                )
                .on_click(cx.listener(|this, _, _, cx| this.restore_ignored_leaks(cx)))
                .into_any_element(),
            );
        }
        self.setting_text(
            "leakAlerts",
            "WarningIcon",
            "settings.leaks.title",
            "settings.leaks.desc",
            controls,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{Filter, annotate, program, row_kind};
    use serde_json::json;

    fn sample() -> serde_json::Value {
        json!({
            "processes": [
                {"pid": 10, "createTime": 1, "name": "agent.exe", "path": "C:\\Contoso\\Agent.exe"},
                {"pid": 11, "createTime": 2, "name": "launcher.exe", "path": "C:\\Contoso\\Launcher.exe"},
                {"pid": 12, "createTime": 3, "name": "bash.exe", "path": "C:\\Tools\\bash.exe"},
                {"pid": 13, "createTime": 4, "name": "bash.exe", "path": "C:\\Tools\\bash.exe"},
                {"pid": 10, "createTime": 9, "name": "agent.exe", "path": "C:\\Contoso\\Agent.exe"},
                {"pid": 14, "createTime": 5, "name": "other.exe", "leak": "handles"}
            ],
            "leaks": [
                {"kind": "handles", "evidence": "extreme", "pid": 10, "createTime": 1, "name": "agent.exe", "path": "C:\\Contoso\\Agent.exe", "count": 250000, "members": []},
                {"kind": "children", "evidence": "growing", "pid": 11, "createTime": 2, "name": "launcher.exe", "path": "C:\\Contoso\\Launcher.exe", "count": 30, "members": []},
                {"kind": "orphans", "evidence": "idle", "pid": 12, "createTime": 3, "name": "bash.exe", "path": "C:\\Tools\\bash.exe", "count": 2,
                 "members": [{"pid": 12, "createTime": 3}, {"pid": 13, "createTime": 4}]},
                {"kind": "future", "pid": 1, "createTime": 1}
            ]
        })
    }

    fn marks(model: &serde_json::Value) -> Vec<Option<&str>> {
        model["processes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["leak"].as_str())
            .collect()
    }

    #[test]
    fn marks_the_instances_each_finding_concerns() {
        let mut model = sample();
        annotate(
            &mut model,
            &Filter {
                enabled: true,
                ..Default::default()
            },
        );
        // A reused PID is another instance; a stale mark goes; an unknown kind is left out.
        assert_eq!(
            marks(&model),
            [
                Some("handles"),
                Some("children"),
                Some("orphans"),
                Some("orphans"),
                None,
                None
            ]
        );
        assert_eq!(model["leakAlerts"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn ignored_programs_and_the_switch_hide_findings_again_and_again() {
        let mut model = sample();
        let mut filter = Filter {
            enabled: true,
            ..Default::default()
        };
        filter.ignored.insert(program(&model["leaks"][0]));
        annotate(&mut model, &filter);
        assert_eq!(marks(&model)[0], None);
        assert_eq!(model["leakAlerts"].as_array().unwrap().len(), 2);
        // The same sample with another filter: it starts again from `leaks`.
        filter.ignored.clear();
        annotate(&mut model, &filter);
        assert_eq!(marks(&model)[0], Some("handles"));
        annotate(&mut model, &Filter::default());
        assert!(marks(&model).iter().all(Option::is_none));
        assert!(model["leakAlerts"].as_array().unwrap().is_empty());
    }

    #[test]
    fn a_row_shows_the_first_kind_among_its_processes() {
        let members = [
            json!({"leak": "orphans"}),
            json!({}),
            json!({"leak": "handles"}),
        ];
        assert_eq!(row_kind(&members), Some("handles"));
        assert_eq!(row_kind(&[json!({})]), None);
    }
}
