//! What a table cell shows: values, marks and the name column's icon and
//! detail, and the hints the row under the pointer reports.
use super::*;

impl NativeTable {
    pub(super) fn td_content(
        &mut self,
        row_index: usize,
        column: usize,
        _window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> AnyElement {
        let row = &self.rows[row_index];
        let field = &self.fields[column];
        let p = Palette::current(cx);
        let faint = p.faint;
        let bits = self.bits;
        let value = self
            .cell_values
            .entry((row_index, field.key))
            .or_insert_with(|| display_cell(row, field, bits))
            .clone();
        let owner = self.owner.clone();
        let mut element = div()
            .text_size(cx.theme().font_size)
            .line_height(theme::line_height())
            .size_full()
            .px(px(CELL_PADDING))
            .flex()
            .items_center()
            .overflow_hidden()
            .whitespace_nowrap()
            .when(field.end, |d| d.justify_end());
        let processes = self.page == "processes";
        if processes
            && row.section.is_none()
            && matches!(
                field.kind,
                CellKind::Percent | CellKind::Memory | CellKind::Rate | CellKind::NetworkRate
            )
            && let Some(value) = row.metric(field.key)
        {
            element = element.bg(p.heat.opacity(theme::heat_alpha(field.key, value)));
        }
        if column == 0 {
            return self.name_cell(row_index, element, cx).into_any_element();
        }
        if row.section.is_some() {
            // Folded headings show their totals a step brighter.
            let collapsed = !row.expanded;
            return element
                .text_size(theme::font(12.))
                .text_color(if collapsed { p.muted } else { faint })
                .child(value)
                .into_any_element();
        }
        let dot = |color: Hsla| div().size(px(7.)).rounded_full().flex_shrink_0().bg(color);
        // The interface's `title` on this cell, for the row under the pointer.
        let hint = self
            .is_near(row_index)
            .then(|| self.cell_hint(row, field, cx))
            .flatten();
        match field.kind {
            CellKind::Enabled => {
                let enabled = boolean(&row.raw, "enabled");
                let blocked = !row.raw["blocked"].is_null();
                let id = row.raw["id"].clone();
                let switch =
                    gpui_kit::component::switch::Switch::new(("startup-toggle", row_index))
                        .checked(enabled)
                        .disabled(blocked)
                        .small()
                        .accessibility_label(if enabled {
                            tr!("startup.toggle.disable", name = text(&row.raw, "name"))
                        } else {
                            tr!("startup.toggle.enable", name = text(&row.raw, "name"))
                        })
                        .on_click(move |_, window, cx| {
                            let owner = owner.clone();
                            let id = id.clone();
                            window.defer(cx, move |window, cx| {
                                let _ = owner.update(cx, |this, cx| {
                                    this.act(
                                        Action::Invoke(
                                            "set_startup_enabled".into(),
                                            json!({"id": id, "enabled": !enabled}),
                                        ),
                                        window,
                                        cx,
                                    )
                                });
                            });
                        });
                let state = div().when(!enabled, |d| d.text_color(faint)).child(value);
                element = match hint {
                    // A blocked switch says why over itself and its state (`.switch-cell`).
                    Some(hint) => element.child(
                        self.hinted(
                            div().flex().items_center().gap_2().min_w_0(),
                            row,
                            field.key,
                            hint,
                        )
                        .child(switch)
                        .child(state),
                    ),
                    None => element.gap_2().child(switch).child(state),
                };
            }
            CellKind::ServiceState => {
                let state = text(&row.raw, "state");
                element = element
                    .gap(px(7.))
                    .child(dot(if state == "running" {
                        p.network
                    } else if state.ends_with("Pending") {
                        p.io
                    } else {
                        faint
                    }))
                    .child(value);
            }
            CellKind::ConnectionState => {
                let state = text(&row.raw, "state");
                element = element.gap(px(7.)).child(dot(match state {
                    "established" => p.network,
                    "listen" => p.cpu,
                    _ => faint,
                }));
                if !value.is_empty() {
                    element = element.child(value);
                }
            }
            CellKind::StartType => {
                element = element
                    .when(text(&row.raw, "startType") == "manual", |d| {
                        d.text_color(faint)
                    })
                    .when(text(&row.raw, "startType") == "disabled", |d| {
                        d.text_color(p.io)
                    })
                    .child(value);
            }
            CellKind::Protocol => {
                element = element
                    .gap(px(6.))
                    .child(value)
                    .when(boolean(&row.raw, "ipv6"), |d| {
                        d.child(tag("v6", p.stroke_strong, p.muted))
                    });
            }
            CellKind::Mono | CellKind::MonoDim | CellKind::Port | CellKind::PortDim => {
                element = element
                    .font_family(theme::mono_font(cx))
                    .text_size(theme::font(12.))
                    .when(
                        matches!(field.kind, CellKind::MonoDim | CellKind::PortDim),
                        |d| d.text_color(faint),
                    )
                    .child(self.hint_text(div().truncate().child(value), row, field.key, hint));
            }
            CellKind::Dim
            | CellKind::ServicePid
            | CellKind::StartupLocation
            | CellKind::ProcessText => {
                element = element.text_color(faint).child(self.hint_text(
                    div().truncate().child(value),
                    row,
                    field.key,
                    hint,
                ));
            }
            CellKind::Pid | CellKind::Identifier => {
                element = element.text_color(faint).child(value);
            }
            _ => {
                // Idle readings stay quiet so real load stands out.
                let reading = row.metric(field.key);
                let dim = match field.kind {
                    CellKind::Percent => reading.is_none_or(|v| v < 0.05),
                    CellKind::Rate | CellKind::NetworkRate => reading.is_none_or(|v| v < 1.),
                    _ => false,
                };
                let delayed = row.native_pending(field.key)
                    && (row.members.iter().any(|member| boolean(member, "stale"))
                        || row.measurement(field.key).is_some_and(|state| {
                            state.missing & argus_core::measurement::STALE != 0
                        }));
                element = element
                    .when(dim || delayed, |d| d.text_color(faint))
                    .child(value);
                // A figure's `title` covers its whole cell.
                element = self.hint_text(element, row, field.key, hint);
            }
        }
        element.into_any_element()
    }
}
fn icon_id(raw: &Value) -> Option<u32> {
    raw["webView"]["hostIcon"]
        .as_u64()
        .or_else(|| raw["icon"].as_u64())
        .map(|id| id as u32)
}
/// A small outlined label, with the shared tag style.
fn tag(text: impl Into<SharedString>, border: Hsla, color: Hsla) -> Div {
    div()
        .flex_shrink_0()
        .px(px(6.))
        .border_1()
        .border_color(border)
        .rounded_full()
        .text_size(theme::font(11.))
        .line_height(px(16.))
        .text_color(color)
        .child(text.into())
}
impl NativeTable {
    /// Whether the row reports its hinted parts in this render (`tips`).
    pub(super) fn is_near(&self, row_index: usize) -> bool {
        self.near == Some(row_index)
    }
    /// `element` as the part `part` of `row`, hinted by `hint`.
    pub(super) fn hinted(&self, element: Div, row: &Row, part: &str, hint: String) -> Div {
        self.tips
            .target(element, tips::id((&self.page, &row.key, part)), hint)
    }
    pub(super) fn hint_text(
        &self,
        element: Div,
        row: &Row,
        part: &str,
        hint: Option<String>,
    ) -> Div {
        match hint {
            Some(hint) => self.hinted(element, row, part, hint),
            None => element,
        }
    }
    /// The hover hint for a cell other than the first, if any.
    pub(super) fn cell_hint(&self, row: &Row, field: &Field, cx: &App) -> Option<String> {
        let raw = &row.raw;
        if self.page == "processes"
            && let Some(state) = row.measurement(field.key)
        {
            let mut parts = Vec::new();
            if state.partial {
                parts.push(label("history.coverage.partial"));
            }
            for (flag, key) in [
                (
                    argus_core::measurement::UNAVAILABLE,
                    "history.coverage.reasons.unavailable",
                ),
                (
                    argus_core::measurement::ACCESS_DENIED,
                    "history.coverage.reasons.denied",
                ),
                (
                    argus_core::measurement::STALE,
                    "history.coverage.reasons.stale",
                ),
                (
                    argus_core::measurement::BASELINE,
                    "history.coverage.reasons.baseline",
                ),
            ] {
                if state.missing & flag != 0 {
                    parts.push(label(key));
                }
            }
            if !parts.is_empty() {
                return Some(parts.join(" · "));
            }
        }
        if self.page == "processes"
            && row.native_pending(field.key)
            && row.metric(field.key).is_none()
            && row.members.iter().any(|p| boolean(p, "countersPending"))
        {
            return Some(label("processes.countersPending"));
        }
        if self.page == "processes"
            && row.native_pending(field.key)
            && row.metric(field.key).is_some()
        {
            return Some(label("processes.refreshPending"));
        }
        match (self.page.as_str(), field.key) {
            // Why a process has no disk reading. Working it out reads every
            // process, so once a sample, as the interface's memo.
            ("processes", "diskRead" | "diskWrite") if row.metric(field.key).is_none() => {
                let owner = self.owner.upgrade()?;
                let app = owner.read(cx);
                let mut cached = self.disk_hint.borrow_mut();
                if cached
                    .as_ref()
                    .is_none_or(|(sample, _)| *sample != app.sequence)
                {
                    *cached = Some((app.sequence, app.missing_disk_hint()));
                }
                cached.as_ref().map(|(_, hint)| hint.clone())
            }
            // The busiest engine, which the figure is the share of.
            ("processes", "gpu") => row.gpu_usage().engine.clone().filter(|e| !e.is_empty()),
            ("processes", "net") => {
                let owner = self.owner.upgrade()?;
                let bits = owner
                    .read(cx)
                    .prefs
                    .get::<String>("netUnit", "bytes".into())
                    == "bits";
                let rate = |key| charts::net_rate(row.metric(key).unwrap_or(0.), bits);
                Some(tr!(
                    "processes.netRates",
                    sent = rate("netSent"),
                    received = rate("netReceived")
                ))
            }
            // The process a running service lives in.
            ("services", "pid") => {
                let pid = raw["pid"].as_u64().filter(|pid| *pid != 0)?;
                let owner = self.owner.upgrade()?;
                let app = owner.read(cx);
                Some(text(app.process(pid)?, "name").to_owned()).filter(|name| !name.is_empty())
            }
            ("services", "description") | ("startup", "command") => {
                Some(text(raw, field.key).to_owned()).filter(|hint| !hint.is_empty())
            }
            ("processes", _) if field.kind == CellKind::ProcessText => {
                row.column_text(field.key).map(str::to_owned)
            }
            ("startup", "enabled") => raw["blocked"]
                .as_str()
                .map(|reason| label(&format!("startup.blocked.{reason}"))),
            _ => None,
        }
    }
    /// The `title` of the first cell's name: the whole name and what it runs.
    pub(super) fn name_hint(&self, row: &Row, cx: &App) -> Option<String> {
        let raw = &row.raw;
        let hint = match self.page.as_str() {
            "processes" => format!(
                "{}\n{}",
                row.title,
                raw["path"].as_str().unwrap_or(text(raw, "name"))
            ),
            "services" => row.title.clone(),
            "startup" => [text(raw, "name"), text(raw, "entry")]
                .into_iter()
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join("\n"),
            // The executable of the connection's process.
            "network" => {
                let owner = self.owner.upgrade()?;
                let app = owner.read(cx);
                app.process(raw["pid"].as_u64()?)?["path"]
                    .as_str()?
                    .to_owned()
            }
            _ => return None,
        };
        (!hint.is_empty()).then_some(hint)
    }
    /// The first cell: fold control, icon, name and the marks that follow it.
    pub(super) fn name_cell(
        &self,
        row_index: usize,
        element: Div,
        cx: &mut Context<TableState<Self>>,
    ) -> Div {
        let row = &self.rows[row_index];
        let p = Palette::current(cx);
        let owner = self.owner.clone();
        let near = self.is_near(row_index);
        let fold = |expanded: bool, key: String, owner: WeakEntity<Desktop>| {
            let hover = p.hover;
            let ink = p.ink;
            Button::new(("fold", row_index))
                .size(px(18.))
                .flex_shrink_0()
                .rounded(px(3.))
                .flex()
                .items_center()
                .justify_center()
                .hover(move |s| s.bg(hover).text_color(ink))
                .child(
                    icon("ChevronRight", p.muted)
                        .size(px(12.))
                        .when(expanded, |i| {
                            i.rotate(gpui_kit::gpui::radians(std::f32::consts::FRAC_PI_2))
                        }),
                )
                // Its press never reaches the row: the row's double-click does
                // not fold again, and every click folds, whatever its count.
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(move |_, window, cx| {
                    let owner = owner.clone();
                    let key = key.clone();
                    window.defer(cx, move |window, cx| {
                        let _ =
                            owner.update(cx, |this, cx| this.act(Action::Fold(key), window, cx));
                    });
                })
        };
        // Rows of a user's group stand in from its heading.
        let inset = if row.nested { USER_INSET } else { 0. };
        if let Some(user) = row.user {
            let locked = self
                .owner
                .upgrade()
                .is_some_and(|owner| !owner.read(cx).query.trim().is_empty());
            return element
                .pl(px(6.))
                .gap_2()
                .child(div().when(locked, |d| d.opacity(0.4)).child(fold(
                    row.expanded,
                    row.key.clone(),
                    owner,
                )))
                .child(
                    icon(
                        if user.system {
                            "ComputerIcon"
                        } else {
                            "UserIcon"
                        },
                        p.muted,
                    )
                    .flex_shrink_0(),
                )
                .child(
                    div()
                        .flex_shrink(1.)
                        .min_w_0()
                        .truncate()
                        .text_size(theme::font(14.))
                        .font_weight(theme::LABEL_WEIGHT)
                        .text_color(p.ink)
                        .child(row.title.clone()),
                )
                // How many processes, and whether the user is disconnected.
                .child(
                    div()
                        .flex_shrink_0()
                        .text_size(theme::font(12.))
                        .text_color(p.faint)
                        .child(row.detail.clone()),
                );
        }
        if let Some(section) = &row.section {
            let locked = self
                .owner
                .upgrade()
                .is_some_and(|owner| !owner.read(cx).query.trim().is_empty());
            let _ = section;
            return element
                .pl(px(6. + inset))
                .gap_2()
                .child(div().when(locked, |d| d.opacity(0.4)).child(fold(
                    row.expanded,
                    row.key.clone(),
                    owner,
                )))
                .child(
                    div()
                        .flex_shrink(1.)
                        .min_w_0()
                        .truncate()
                        .text_size(theme::font(14.))
                        .font_weight(theme::LABEL_WEIGHT)
                        .text_color(p.ink)
                        .child(row.title.clone()),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .min_w(px(22.))
                        .px(px(7.))
                        .rounded_full()
                        .bg(p.accent.opacity(0.14))
                        .text_color(p.accent.blend(p.ink.opacity(0.22)))
                        .text_size(theme::font(11.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .line_height(px(18.))
                        .flex()
                        .justify_center()
                        .child(row.raw["entryCount"].to_string()),
                )
                .child(div().flex_1().min_w_0().h(px(1.)).ml_1().bg(
                    gpui_kit::gpui::linear_gradient(
                        90.,
                        gpui_kit::gpui::linear_color_stop(p.stroke_strong, 0.),
                        gpui_kit::gpui::linear_color_stop(p.stroke_strong.opacity(0.), 1.),
                    ),
                ));
        }
        let processes = self.page == "processes";
        let mut element = element.gap_2();
        if processes {
            element = element.pl(px(6. + inset + row.depth as f32 * 20.));
            element = if row.expandable {
                element.child(
                    div()
                        .mr(px(-4.))
                        .child(fold(row.expanded, row.key.clone(), owner)),
                )
            } else {
                element.child(div().w(px(14.)).flex_shrink_0())
            };
        }
        if self.page != "services" {
            let image =
                icon_id(&row.raw).and_then(|id| self.owner.upgrade()?.read(cx).process_image(id));
            let kernel = processes
                && !boolean(&row.raw, "countersPending")
                && row.raw["path"].is_null()
                && row.members.len() <= 1;
            element = element.child(div().relative().size_4().flex_shrink_0().child(
                if let Some(image) = image {
                    img(image).size_4().into_any_element()
                } else {
                    icon(if kernel { "KernelIcon" } else { "AppIcon" }, p.glyph).into_any_element()
                },
            ));
        }
        let group = processes && row.members.len() > 1;
        let off = self.page == "startup" && !boolean(&row.raw, "enabled");
        let name = div()
            .min_w_0()
            .flex_shrink(1.)
            .truncate()
            .when(group, |d| d.font_weight(theme::LABEL_WEIGHT))
            .when(processes && boolean(&row.raw, "suspended"), |d| {
                d.text_color(p.muted)
            })
            .when(off, |d| d.text_color(p.muted))
            .child(row.title.clone());
        // The marks after the name carry their own `title`s.
        let hint = |element: Div, part: &str, hint: Option<String>| {
            if near {
                self.hint_text(element, row, part, hint)
            } else {
                element
            }
        };
        element = element.child(hint(
            name,
            "name",
            near.then(|| self.name_hint(row, cx)).flatten(),
        ));
        if group {
            element = element.child(
                div()
                    .flex_shrink_0()
                    .min_w(px(20.))
                    .px(px(6.))
                    .rounded_full()
                    .bg(p.hover)
                    .text_color(p.muted)
                    .text_size(theme::font(11.))
                    .line_height(px(17.))
                    .flex()
                    .justify_center()
                    .child(row.members.len().to_string()),
            );
        }
        if let Some(note) = &row.note {
            let badge = div()
                .flex()
                .flex_shrink(1.)
                .items_center()
                .gap_1()
                .min_w(px(40.))
                .h(px(20.))
                .ml(px(-2.))
                .pl(px(5.))
                .pr(px(7.))
                .rounded(px(4.))
                .bg(p.accent.opacity(0.11))
                .text_color(p.accent.blend(p.ink.opacity(0.28)))
                .text_size(theme::font(12.))
                .child(
                    icon("NoteIcon", p.accent.blend(p.ink.opacity(0.28)))
                        .size(px(12.))
                        .flex_shrink_0(),
                )
                .child(div().min_w_0().truncate().child(note.clone()));
            element = element.child(hint(badge, "note", Some(note.clone())));
        }
        // The image name gives way first in a narrow column, cut short with
        // an ellipsis before the name is; it does not fill the column, so the
        // marks after it stay beside the text however wide the column is.
        if processes && !row.detail.is_empty() {
            element = element.child(
                div()
                    .flex_shrink(1000.)
                    .min_w_0()
                    .truncate()
                    .text_color(p.faint)
                    .child(row.detail.clone()),
            );
        }
        if processes {
            let efficient = row
                .members
                .iter()
                .filter(|member| boolean(member, "efficiency"))
                .count();
            if efficient > 0 {
                let leaf = icon("LeafIcon", p.muted).size(px(12.)).flex_shrink_0();
                element = if near {
                    element.child(hint(
                        div().flex().flex_shrink_0().child(leaf),
                        "efficiency",
                        Some(if group {
                            tr!("processes.efficiencyCount", count = efficient)
                        } else {
                            label("common.efficiencyMode")
                        }),
                    ))
                } else {
                    element.child(leaf)
                };
            }
            if row
                .members
                .iter()
                .all(|member| boolean(member, "suspended"))
            {
                element =
                    element.child(tag(label("processes.suspended"), p.stroke_strong, p.muted));
            }
        }
        if self.page == "startup" && !boolean(&row.raw, "exists") && !row.raw["path"].is_null() {
            let missing = tag(label("startup.missing.label"), p.io.opacity(0.55), p.io);
            element = element.child(hint(
                missing,
                "missing",
                Some(label("startup.missing.title")),
            ));
        }
        if row.pinned {
            let pin = icon("PinIcon", p.muted).flex_shrink_0();
            // The other lists' names fill their column, and the pin marks the
            // end of it; a process name's column fits it, the pin follows.
            if !processes {
                element = element.child(div().flex_1());
            }
            element = if near {
                element.child(hint(
                    div().flex().flex_shrink_0().child(pin),
                    "pin",
                    Some(label("common.marks.pinned")),
                ))
            } else {
                element.child(pin)
            };
        }
        element
    }
}
