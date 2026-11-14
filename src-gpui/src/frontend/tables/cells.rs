//! What a table cell shows: values, marks and the name column's icon and
//! detail, and the hints the row under the pointer reports.
use super::*;
use crate::frontend::plain::{Plain, Prepainted, plain, plain_text};

/// A name's data-dependent marks, shared by every frame of the same row list.
/// Page-specific visibility remains in `name_cell`; these facts have no theme
/// or language dependencies and contain no process-image handles.
pub(super) struct NameFacts {
    /// Members in efficiency mode, including those hidden by a folded group.
    pub efficiency_count: usize,
    /// Every member is suspended. An empty list preserves `Iterator::all`.
    pub all_suspended: bool,
    /// The representative process dims the name independently of the group tag.
    pub suspended: bool,
    /// Kernel fallback icon eligibility; used only on the processes page.
    pub kernel: bool,
    /// A WebView host icon takes precedence over the executable's icon.
    pub icon_id: Option<u32>,
    /// Group membership, and its already formatted count badge.
    pub members: usize,
    pub member_count: SharedString,
    /// Category count as JSON renders it, including missing/malformed values.
    pub entry_count: SharedString,
    /// Startup name dimming and the missing-target badge, respectively.
    pub startup_disabled: bool,
    pub startup_missing: bool,
    /// The leak alert the row carries (`leaks::row_kind`).
    pub leak: Option<&'static str>,
}

impl NameFacts {
    fn of(row: &Row) -> Self {
        let mut efficiency_count = 0;
        let mut all_suspended = true;
        for member in &row.members {
            efficiency_count += usize::from(boolean(member, "efficiency"));
            all_suspended &= boolean(member, "suspended");
        }
        Self {
            efficiency_count,
            all_suspended,
            suspended: boolean(&row.raw, "suspended"),
            kernel: !boolean(&row.raw, "countersPending")
                && row.raw["path"].is_null()
                && row.members.len() <= 1,
            icon_id: icon_id(&row.raw),
            members: row.members.len(),
            member_count: row.members.len().to_string().into(),
            entry_count: row.raw["entryCount"].to_string().into(),
            startup_disabled: !boolean(&row.raw, "enabled"),
            startup_missing: !boolean(&row.raw, "exists") && !row.raw["path"].is_null(),
            leak: crate::frontend::leaks::row_kind(&row.members),
        }
    }
}

/// Everything that can change the cell palette or the font overrides.
/// The first four fields match `Palette::current`'s dependency key.
#[derive(PartialEq)]
struct StyleKey {
    mode: ThemeMode,
    accent: Hsla,
    foreground: Hsla,
    background: Hsla,
    font_size: Pixels,
    small_font: Pixels,
    font_family: SharedString,
    mono_font: SharedString,
    locale: argus_core::i18n::Locale,
}

impl StyleKey {
    fn current(cx: &App) -> Self {
        let theme = cx.theme();
        Self {
            mode: theme.mode,
            accent: theme.colors.primary,
            foreground: theme.colors.foreground,
            background: theme.colors.background,
            font_size: theme.font_size,
            small_font: theme::font(12.),
            font_family: theme.font_family.clone(),
            mono_font: theme::mono_font(cx),
            locale: argus_core::i18n::locale(),
        }
    }
}

struct CellStyle {
    key: StyleKey,
    palette: Palette,
}

/// Where a lazily requested hint belongs. The hint text and target ID are
/// created only for the row under the pointer, never stored in the cache.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HintArea {
    None,
    Text,
    Whole,
    SectionMemory,
}

/// Compact display specification: no layout state or `TextStyleRefinement`.
/// Unset colors/families retain the text style inherited from the cell's box.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct CellSpec {
    pub text: SharedString,
    color: Option<Hsla>,
    background: Option<Hsla>,
    font_family: Option<SharedString>,
    font_size: Pixels,
    truncate: bool,
    end: bool,
    hint_area: HintArea,
}

impl CellSpec {
    fn of(row: &Row, field: &Field, bits: bool, processes: bool, style: &CellStyle) -> Self {
        let p = &style.palette;
        let mut spec = Self {
            text: display_cell(row, field, bits).into(),
            color: None,
            background: None,
            font_family: None,
            font_size: style.key.font_size,
            truncate: false,
            end: field.end,
            hint_area: HintArea::None,
        };
        if row.section.is_some() {
            spec.color = Some(if row.expanded { p.faint } else { p.muted });
            spec.font_size = style.key.small_font;
            if processes && field.key == "mem" {
                spec.hint_area = HintArea::SectionMemory;
            }
            return spec;
        }
        match field.kind {
            CellKind::Name
            | CellKind::Enabled
            | CellKind::ServiceState
            | CellKind::ConnectionState
            | CellKind::Protocol => return spec,
            CellKind::Mono | CellKind::MonoDim | CellKind::Port | CellKind::PortDim => {
                spec.font_family = Some(style.key.mono_font.clone());
                spec.font_size = style.key.small_font;
                spec.truncate = true;
                spec.hint_area = HintArea::Text;
                if matches!(field.kind, CellKind::MonoDim | CellKind::PortDim) {
                    spec.color = Some(p.faint);
                }
            }
            CellKind::Dim
            | CellKind::ServicePid
            | CellKind::StartupLocation
            | CellKind::ProcessText
            | CellKind::Pid
            | CellKind::Identifier => {
                spec.color = Some(p.faint);
                spec.truncate = true;
                spec.hint_area = HintArea::Text;
            }
            CellKind::StartType => {
                // A start type pending a restart shows as what it will be.
                let start = match text(&row.raw, "pendingStartType") {
                    "" => text(&row.raw, "startType"),
                    pending => pending,
                };
                spec.color = match start {
                    "manual" => Some(p.faint),
                    "disabled" => Some(p.io),
                    _ => None,
                };
            }
            _ => {
                spec.hint_area = HintArea::Whole;
                // Reuse this reading for both the idle tone and the heat fill.
                let reading = row.metric(field.key);
                let dim = match field.kind {
                    CellKind::Percent => reading.is_none_or(|v| v < 0.05),
                    CellKind::Rate | CellKind::NetworkRate => reading.is_none_or(|v| v < 1.),
                    CellKind::Power => reading.is_none_or(|v| columns::power_level(v) == 0),
                    _ => false,
                };
                let delayed = row.native_pending(field.key)
                    && row.members.iter().any(|member| boolean(member, "stale"));
                if dim || delayed {
                    spec.color = Some(p.faint);
                }
                // The count a handle leak alert is about, in its amber.
                if processes
                    && field.key == "handles"
                    && row.members.iter().any(|m| text(m, "leak") == "handles")
                {
                    spec.color = Some(p.io);
                }
                if processes
                    && matches!(
                        field.kind,
                        CellKind::Percent
                            | CellKind::Memory
                            | CellKind::Rate
                            | CellKind::NetworkRate
                            | CellKind::Power
                    )
                {
                    spec.background =
                        reading.map(|value| p.heat.opacity(theme::heat_alpha(field.key, value)));
                }
            }
        }
        spec
    }

    fn cell(&self) -> cell_text::CellText {
        let mut cell =
            cell_text::cell_text(self.text.clone(), self.font_size, px(CELL_PADDING)).end(self.end);
        if let Some(color) = self.color {
            cell = cell.color(color);
        }
        if let Some(background) = self.background {
            cell = cell.background(background);
        }
        if let Some(family) = &self.font_family {
            cell = cell.font_family(family.clone());
        }
        if self.truncate {
            cell = cell.truncate();
        }
        cell
    }
}

/// Display values shared by drawing and automatic widths. Row identity owns
/// the name facts; units, page, locale and style also invalidate cell specs.
#[derive(Default)]
pub(super) struct CellValues {
    source: std::sync::Weak<Vec<Row>>,
    page: String,
    bits: bool,
    style: Option<CellStyle>,
    values: HashMap<(usize, &'static str), CellSpec>,
    names: HashMap<usize, NameFacts>,
}

impl CellValues {
    pub(super) fn configure(&mut self, rows: &Arc<Vec<Row>>, page: &str, bits: bool, cx: &App) {
        if !std::ptr::eq(self.source.as_ptr(), Arc::as_ptr(rows)) {
            self.values.clear();
            self.names.clear();
            self.source = Arc::downgrade(rows);
        }
        if self.page != page || self.bits != bits {
            self.values.clear();
            self.page = page.into();
            self.bits = bits;
        }
        self.prepare(cx);
    }

    /// Called before width measurement and once before either body renders.
    pub(super) fn prepare(&mut self, cx: &App) {
        let key = StyleKey::current(cx);
        if self.style.as_ref().is_none_or(|style| style.key != key) {
            self.values.clear();
            self.style = Some(CellStyle {
                key,
                palette: Palette::current(cx),
            });
        }
    }

    fn style(&self) -> &CellStyle {
        self.style
            .as_ref()
            .expect("cell styles prepared before rows")
    }

    pub(super) fn get(&mut self, index: usize, row: &Row, field: &Field) -> &CellSpec {
        let style = self
            .style
            .as_ref()
            .expect("cell styles prepared before rows");
        self.values
            .entry((index, field.key))
            .or_insert_with(|| CellSpec::of(row, field, self.bits, self.page == "processes", style))
    }

    /// Initialize after the row list is installed, once even for large groups.
    fn prepare_name(&mut self, index: usize, row: &Row) {
        self.names
            .entry(index)
            .or_insert_with(|| NameFacts::of(row));
    }
}

/// What a cell shows: one line of text, which a row that draws its own cells
/// places and paints itself (`body`), or elements.
// Text is what most cells are: boxed, every one of them would allocate on
// every frame.
#[allow(clippy::large_enum_variant)]
pub(super) enum Content {
    Text(cell_text::CellText),
    Element(AnyElement),
}

impl NativeTable {
    pub(super) fn td_content(
        &mut self,
        row_index: usize,
        column: usize,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> AnyElement {
        match self.cell(row_index, column, window, cx) {
            Content::Text(text) => text.into_any_element(),
            Content::Element(element) => element,
        }
    }

    pub(super) fn cell(
        &mut self,
        row_index: usize,
        column: usize,
        _window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> Content {
        let row = &self.rows[row_index];
        let field = &self.fields[column];
        // A cell that shows one line of text is one element (`cell_text`).
        let marked = matches!(
            field.kind,
            CellKind::Enabled
                | CellKind::ServiceState
                | CellKind::ConnectionState
                | CellKind::Protocol
        );
        if column > 0 && (row.section.is_some() || !marked) {
            let spec = self.cell_values.get(row_index, row, field);
            let cell = spec.cell();
            let hint_area = spec.hint_area;
            return Content::Text(self.text_cell(row_index, row, field, cell, hint_area, cx));
        }
        let owner = self.owner.clone();
        let mut element = self.content_box(field.end);
        if column == 0 {
            return Content::Element(self.name_cell(row_index, element, cx).into_any_element());
        }
        let value = self.cell_values.get(row_index, row, field).text.clone();
        let p = &self.cell_values.style().palette;
        let faint = p.faint;
        let dot = |color: Hsla| {
            plain()
                .size(px(7.))
                .rounded_full()
                .flex_shrink_0()
                .bg(color)
        };
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
                let state = plain().when(!enabled, |d| d.text_color(faint)).child(value);
                element = match hint {
                    // A blocked switch says why over itself and its state (`.switch-cell`).
                    Some(hint) => element.child(
                        self.hinted(
                            plain().flex().items_center().gap_2().min_w_0(),
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
            CellKind::Protocol => {
                element = element
                    .gap(px(6.))
                    .child(value)
                    .when(boolean(&row.raw, "ipv6"), |d| {
                        d.child(tag("v6", p.stroke_strong, p.muted))
                    });
            }
            // Every other kind is text (`text_cell`).
            _ => element = element.child(value),
        }
        Content::Element(element.into_any_element())
    }
}
fn icon_id(raw: &Value) -> Option<u32> {
    raw["webView"]["hostIcon"]
        .as_u64()
        .or_else(|| raw["icon"].as_u64())
        .map(|id| id as u32)
}
/// A small outlined label, with the shared tag style.
fn tag(text: impl Into<SharedString>, border: Hsla, color: Hsla) -> Plain {
    plain()
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
    /// The box of a cell that is not one line of text: the cell's padding and
    /// text, its marks in a row. Plain boxes: nothing in a cell listens but
    /// its switch and its fold control, which are elements of their own.
    fn content_box(&self, end: bool) -> Plain {
        plain()
            .text_size(self.cell_values.style().key.font_size)
            .line_height(theme::line_height())
            .size_full()
            .px(px(CELL_PADDING))
            .flex()
            .items_center()
            .overflow_hidden()
            .whitespace_nowrap()
            .when(end, |d| d.justify_end())
    }

    /// The name cell of a row Argus draws (`body`), laid out on its own:
    /// the component's box around a cell (`render_cell`) and the cell's own
    /// box are the same size and clip alike, so they are one box here, the
    /// column's width across the row.
    pub(super) fn name_root(
        &mut self,
        row_index: usize,
        size: Size<Pixels>,
        cx: &mut Context<TableState<Self>>,
    ) -> AnyElement {
        let element = self
            .content_box(false)
            .w(size.width)
            .h(size.height)
            .flex_shrink_0()
            .relative();
        self.name_cell(row_index, element, cx).into_any_element()
    }

    /// Add the current hint to an already styled cell, only under the pointer.
    fn text_cell(
        &self,
        row_index: usize,
        row: &Row,
        field: &Field,
        cell: cell_text::CellText,
        area: HintArea,
        cx: &App,
    ) -> cell_text::CellText {
        if !self.is_near(row_index) || area == HintArea::None {
            return cell;
        }
        let hint = match area {
            HintArea::SectionMemory => Some(label("processes.memoryScope.section")),
            _ => self.cell_hint(row, field, cx),
        };
        match hint {
            Some(hint) => cell.hint(
                &self.tips,
                tips::id((&self.page, &row.key, field.key)),
                hint,
                matches!(area, HintArea::Whole | HintArea::SectionMemory),
            ),
            None => cell,
        }
    }
    /// Whether the row reports its hinted parts in this render (`tips`).
    pub(super) fn is_near(&self, row_index: usize) -> bool {
        self.near == Some(row_index)
    }
    /// `element` as the part `part` of `row`, hinted by `hint`: it reports
    /// where it was laid out as it prepaints, within its clip, as the child
    /// `Tips::target` adds would.
    pub(super) fn hinted<E: Prepainted>(
        &self,
        element: E,
        row: &Row,
        part: &str,
        hint: String,
    ) -> E {
        let tips = self.tips.clone();
        let id = tips::id((&self.page, &row.key, part));
        element.on_prepainted(move |bounds, window, cx| {
            tips.report(id, bounds, hint.into(), window, cx)
        })
    }
    pub(super) fn hint_text<E: Prepainted>(
        &self,
        element: E,
        row: &Row,
        part: &str,
        hint: Option<String>,
    ) -> E {
        match hint {
            Some(hint) => self.hinted(element, row, part, hint),
            None => element,
        }
    }
    /// The hover hint for a cell other than the first, if any.
    pub(super) fn cell_hint(&self, row: &Row, field: &Field, cx: &App) -> Option<String> {
        let raw = &row.raw;
        // A text column (the sampled priority levels) has its value as text.
        let present = row.metric(field.key).is_some()
            || (field.kind == CellKind::ProcessText && row.column_text(field.key).is_some());
        if self.page == "processes"
            && row.native_pending(field.key)
            && !present
            && row.members.iter().any(|p| boolean(p, "countersPending"))
        {
            return Some(label("processes.countersPending"));
        }
        if self.page == "processes" && row.native_pending(field.key) && present {
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
            // Keep the full class name available when a manual width truncates it;
            // custom values instead explain where they sit between the classes.
            ("processes", "basePriority") if row.metric(field.key).is_some() => {
                super::base_priority_hint(&row.members[0])
                    .or_else(|| super::base_priority(&row.members[0]))
            }
            // The busiest engine, which the figure is the share of.
            ("processes", "gpu") => row.gpu_usage().engine.clone().filter(|e| !e.is_empty()),
            // Why the count is in amber.
            ("processes", "handles")
                if row.members.iter().any(|m| text(m, "leak") == "handles") =>
            {
                let owner = self.owner.upgrade()?;
                let app = owner.read(cx);
                app.row_leak(&row.members)
                    .filter(|leak| text(leak, "kind") == "handles")
                    .map(crate::frontend::leaks::describe)
            }
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
            // What the cell shows, localized levels included.
            ("processes", _) if field.kind == CellKind::ProcessText => row
                .column_text(field.key)
                .map(|value| columns::shown(field.key, value).into_owned()),
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
    /// Stable row facts for either element-based or directly drawn names.
    pub(super) fn name_facts(&mut self, row_index: usize) -> &NameFacts {
        self.cell_values
            .prepare_name(row_index, &self.rows[row_index]);
        &self.cell_values.names[&row_index]
    }
    /// The first cell: fold control, icon, name and the marks that follow it.
    pub(super) fn name_cell(
        &mut self,
        row_index: usize,
        element: Plain,
        cx: &mut Context<TableState<Self>>,
    ) -> Plain {
        self.name_facts(row_index);
        let row = &self.rows[row_index];
        let facts = &self.cell_values.names[&row_index];
        let p = &self.cell_values.style().palette;
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
        let tone = self
            .group_view
            .tone(row_index)
            .map_or(p.accent, |tone| tone.color(p));
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
                // The user's mark in the group's colour.
                .child(
                    plain()
                        .size(px(20.))
                        .flex_shrink_0()
                        .rounded_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(tone.opacity(groups::COUNT_TINT))
                        .child(
                            icon(
                                if user.system {
                                    "ComputerIcon"
                                } else {
                                    "UserIcon"
                                },
                                tone,
                            )
                            .size(px(12.)),
                        ),
                )
                .child(
                    plain_text(row.title.clone())
                        .flex_shrink(1.)
                        .min_w_0()
                        .truncate()
                        .text_size(theme::font(14.))
                        .font_weight(theme::LABEL_WEIGHT)
                        .text_color(p.ink),
                )
                // How many processes, and whether the user is disconnected.
                .child(
                    plain_text(row.detail.clone())
                        .flex_shrink_0()
                        .text_size(theme::font(12.))
                        .text_color(p.faint),
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
                    plain()
                        .size(px(8.))
                        .flex_shrink_0()
                        .rounded(px(2.))
                        .bg(tone),
                )
                .child(
                    plain_text(row.title.clone())
                        .flex_shrink(1.)
                        .min_w_0()
                        .truncate()
                        .text_size(theme::font(14.))
                        .font_weight(theme::LABEL_WEIGHT)
                        .text_color(p.ink),
                )
                .child(
                    plain()
                        .flex_shrink_0()
                        .min_w(px(22.))
                        .px(px(7.))
                        .rounded_full()
                        .bg(tone.opacity(groups::COUNT_TINT))
                        .text_color(tone.blend(p.ink.opacity(0.22)))
                        .text_size(theme::font(11.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .line_height(px(18.))
                        .flex()
                        .justify_center()
                        .child(facts.entry_count.clone()),
                )
                .child(plain().flex_1().min_w_0().h(px(1.)).ml_1().bg(
                    gpui_kit::gpui::linear_gradient(
                        90.,
                        gpui_kit::gpui::linear_color_stop(tone.opacity(0.35), 0.),
                        gpui_kit::gpui::linear_color_stop(tone.opacity(0.), 1.),
                    ),
                ));
        }
        let processes = self.page == "processes";
        let mut element = element.gap_2();
        if processes {
            element = element.pl(px(6. + inset + row.depth as f32 * 20.));
            element = if row.expandable {
                // The button carries the margin itself: a wrapper for it would
                // be one more node in every row, laid out on every frame.
                element.child(fold(row.expanded, row.key.clone(), owner).mr(px(-4.)))
            } else {
                element.child(plain().w(px(14.)).flex_shrink_0())
            };
        }
        if self.page != "services" {
            let image = facts
                .icon_id
                .and_then(|id| self.owner.upgrade()?.read(cx).process_image(id));
            let kernel = processes && facts.kernel;
            // An image is the flex item itself, where the box around the
            // fallback icon gives that icon its place.
            element = element.child(if let Some(image) = image {
                img(image).size_4().flex_shrink_0().into_any_element()
            } else {
                plain()
                    .relative()
                    .size_4()
                    .flex_shrink_0()
                    .child(icon(if kernel { "KernelIcon" } else { "AppIcon" }, p.glyph))
                    .into_any_element()
            });
        }
        let group = processes && facts.members > 1;
        let off = self.page == "startup" && facts.startup_disabled;
        let name = plain_text(row.title.clone())
            .min_w_0()
            .flex_shrink(1.)
            .truncate()
            .when(group, |d| d.font_weight(theme::LABEL_WEIGHT))
            .when(processes && facts.suspended, |d| d.text_color(p.muted))
            .when(off, |d| d.text_color(p.muted));
        // The name and the marks after it carry their own `title`s.
        element = element.child(if near {
            self.hint_text(name, row, "name", self.name_hint(row, cx))
        } else {
            name
        });
        let hint = |element: Plain, part: &str, hint: Option<String>| {
            if near {
                self.hint_text(element, row, part, hint)
            } else {
                element
            }
        };
        if group {
            element = element.child(
                plain()
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
                    .child(facts.member_count.clone()),
            );
        }
        if let Some(note) = &row.note {
            let badge = plain()
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
                .child(plain_text(note.clone()).min_w_0().truncate());
            element = element.child(hint(badge, "note", near.then(|| note.clone())));
        }
        // The image name gives way first in a narrow column, cut short with
        // an ellipsis before the name is; it does not fill the column, so the
        // marks after it stay beside the text however wide the column is.
        if processes && !row.detail.is_empty() {
            element = element.child(
                plain_text(row.detail.clone())
                    .flex_shrink(1000.)
                    .min_w_0()
                    .truncate()
                    .text_color(p.faint),
            );
        }
        if processes {
            let efficient = facts.efficiency_count;
            if efficient > 0 {
                let leaf = icon("LeafIcon", p.muted).size(px(12.)).flex_shrink_0();
                element = if near {
                    element.child(hint(
                        plain().flex().flex_shrink_0().child(leaf),
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
            if facts.all_suspended {
                element =
                    element.child(tag(label("processes.suspended"), p.stroke_strong, p.muted));
            }
            // A leak alert: the kind in amber, what it rests on under the pointer.
            if let Some(kind) = facts.leak {
                let mark = tag(
                    crate::frontend::leaks::tag_label(kind),
                    p.io.opacity(0.55),
                    p.io,
                );
                let why = near
                    .then(|| {
                        let owner = self.owner.upgrade()?;
                        let app = owner.read(cx);
                        app.row_leak(&row.members)
                            .map(crate::frontend::leaks::describe)
                    })
                    .flatten();
                element = element.child(hint(mark, "leak", why));
            }
        }
        if self.page == "startup" && facts.startup_missing {
            let missing = tag(label("startup.missing.label"), p.io.opacity(0.55), p.io);
            element = element.child(hint(
                missing,
                "missing",
                near.then(|| label("startup.missing.title")),
            ));
        }
        if row.pinned {
            let pin = icon("PinIcon", p.muted).flex_shrink_0();
            // The other lists' names fill their column, and the pin marks the
            // end of it; a process name's column fits it, the pin follows.
            if !processes {
                element = element.child(plain().flex_1());
            }
            element = if near {
                element.child(hint(
                    plain().flex().flex_shrink_0().child(pin),
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

#[cfg(test)]
mod tests {
    use super::{
        CellKind, CellSpec, CellValues, Field, HintArea, NameFacts, Row, Track, display_cell,
    };
    use gpui_kit::component::{Theme, ThemeMode};
    use gpui_kit::{px, rgb};
    use gpui_test::TestAppContext;
    use serde_json::json;
    use std::sync::Arc;

    fn field(key: &'static str, kind: CellKind) -> Field {
        Field {
            key,
            kind,
            label: String::new(),
            track: Track::Fixed(100.),
            end: true,
        }
    }

    #[test]
    fn cached_cells_survive_frames_and_follow_rows_units_and_pages() {
        let cx = TestAppContext::single();
        cx.update(|cx| {
            cx.set_global(Theme::default());
            let rows = Arc::new(vec![Row::process(
                json!({"pid": 42, "createTime": 1, "netReceived": 2048, "netSent": 1024}),
                0,
            )]);
            let field = field("net", CellKind::NetworkRate);
            let mut cache = CellValues::default();
            cache.configure(&rows, "processes", false, cx);
            let original = cache.get(0, &rows[0], &field).clone();
            cache.prepare_name(0, &rows[0]);
            cache.configure(&rows.clone(), "processes", false, cx);
            assert_eq!(cache.values.len(), 1);
            assert_eq!(cache.names.len(), 1);
            // A hit must not consult the supplied JSON, even for tone or heat.
            let empty = Row::process(json!({}), 0);
            assert_eq!(cache.get(0, &empty, &field), &original);

            cache.configure(&rows, "processes", true, cx);
            assert!(cache.values.is_empty());
            assert_eq!(cache.names.len(), 1);
            assert_eq!(
                cache.get(0, &rows[0], &field).text,
                display_cell(&rows[0], &field, true)
            );

            cache.configure(&rows, "network", true, cx);
            assert!(cache.values.is_empty());
            assert!(cache.get(0, &rows[0], &field).background.is_none());
            assert!(original.background.is_some());

            // A new allocation at the same row index/key replaces every fact.
            let replacement = Arc::new(vec![empty]);
            cache.configure(&replacement, "processes", false, cx);
            assert!(cache.values.is_empty());
            assert!(cache.names.is_empty());
            assert_ne!(cache.get(0, &replacement[0], &field), &original);
        });
    }

    #[test]
    fn header_preparation_invalidates_styles_without_configuring_rows() {
        let cx = TestAppContext::single();
        cx.update(|cx| {
            cx.set_global(Theme::default());
            let rows = Arc::new(vec![Row::process(json!({"pid": 42, "cpu": 15}), 0)]);
            let field = field("cpu", CellKind::Percent);
            let mut cache = CellValues::default();
            cache.configure(&rows, "processes", false, cx);
            cache.prepare_name(0, &rows[0]);
            for change in 0..7 {
                cache.get(0, &rows[0], &field);
                cache.prepare(cx);
                assert_eq!(cache.values.len(), 1);
                match change {
                    0 => Theme::global_mut(cx).colors.primary = rgb(0x765432).into(),
                    1 => Theme::global_mut(cx).mode = ThemeMode::Dark,
                    2 => Theme::global_mut(cx).font_size = px(20.),
                    3 => Theme::global_mut(cx).font_family = "test-family".into(),
                    4 => Theme::global_mut(cx).colors.foreground = rgb(0xabcdef).into(),
                    // Simulate cached metadata from another font scale/locale
                    // without changing process-wide locale or font settings.
                    5 => cache.style.as_mut().unwrap().key.small_font = px(99.),
                    _ => {
                        let locale = cache.style.as_mut().unwrap();
                        locale.key.locale = *argus_core::i18n::Locale::ALL
                            .iter()
                            .find(|locale| **locale != argus_core::i18n::locale())
                            .unwrap();
                    }
                }
                cache.prepare(cx);
                assert!(cache.values.is_empty());
                assert_eq!(cache.names.len(), 1);
                assert_eq!(cache.style().key.font_size, Theme::global(cx).font_size);
            }
        });
    }

    #[test]
    fn display_specs_keep_idle_pending_heading_and_text_styles() {
        let cx = TestAppContext::single();
        cx.update(|cx| {
            cx.set_global(Theme::default());
            let mut cache = CellValues::default();
            cache.prepare(cx);
            let style = cache.style();
            let cpu = field("cpu", CellKind::Percent);
            for (reading, dim) in [(0.049, true), (0.05, false), (19., false)] {
                for stale in [false, true] {
                    let row = Row::process(json!({"cpu": reading, "stale": stale}), 0);
                    let spec = CellSpec::of(&row, &cpu, false, true, style);
                    assert_eq!(spec.color, (dim || stale).then_some(style.palette.faint));
                    assert!(spec.background.is_some());
                    assert!(spec.end && !spec.truncate);
                    assert_eq!(spec.hint_area, HintArea::Whole);
                    assert_eq!(spec.text, display_cell(&row, &cpu, false));
                }
            }
            let mut heading = Row::process(json!({"privateWs": 4096}), 0);
            heading.section = Some("apps".into());
            let memory = field("mem", CellKind::Memory);
            for expanded in [false, true] {
                heading.expanded = expanded;
                let spec = CellSpec::of(&heading, &memory, false, true, style);
                assert_eq!(
                    spec.color,
                    Some(if expanded {
                        style.palette.faint
                    } else {
                        style.palette.muted
                    })
                );
                assert_eq!(spec.font_size, style.key.small_font);
                assert!(spec.background.is_none());
                assert_eq!(spec.hint_area, HintArea::SectionMemory);
            }
            let row = Row::item(
                json!({"startType": "disabled", "localPort": 80}),
                "item".into(),
                String::new(),
            );
            let port = CellSpec::of(
                &row,
                &field("localPort", CellKind::PortDim),
                false,
                false,
                style,
            );
            assert_eq!(port.font_family.as_ref(), Some(&style.key.mono_font));
            assert!(port.truncate);
            assert_eq!(port.color, Some(style.palette.faint));
            assert_eq!(port.hint_area, HintArea::Text);
            let start = CellSpec::of(
                &row,
                &field("startType", CellKind::StartType),
                false,
                false,
                style,
            );
            assert_eq!(start.color, Some(style.palette.io));
            assert_eq!(start.hint_area, HintArea::None);
        });
    }

    #[test]
    fn name_facts_preserve_representative_group_and_startup_distinctions() {
        let mut row = Row::process(
            json!({"icon": 7, "webView": {"hostIcon": 11}, "suspended": true}),
            0,
        );
        row.members
            .push(json!({"efficiency": true, "suspended": false}));
        let facts = NameFacts::of(&row);
        assert_eq!(facts.icon_id, Some(11));
        assert_eq!(facts.efficiency_count, 1);
        assert!(facts.suspended && !facts.all_suspended && !facts.kernel);
        assert_eq!(facts.members, 2);
        assert_eq!(facts.member_count, row.members.len().to_string());
        row.members.pop();
        row.raw["webView"] = json!(null);
        assert_eq!(NameFacts::of(&row).icon_id, Some(7));
        assert!(NameFacts::of(&row).kernel);
        assert!(NameFacts::of(&row).all_suspended);
        row.raw["countersPending"] = json!(true);
        assert!(!NameFacts::of(&row).kernel);
        let item = Row::item(
            json!({"path": "missing.exe", "exists": false, "enabled": false, "entryCount": null}),
            "item".into(),
            String::new(),
        );
        let facts = NameFacts::of(&item);
        assert!(facts.startup_disabled && facts.startup_missing && facts.all_suspended);
        assert_eq!(facts.entry_count, item.raw["entryCount"].to_string());
    }
}
