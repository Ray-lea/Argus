//! The GPU view: which engines it charts (lib/gpu-engines.ts), how their charts are laid
//! out (lib/engine-grid.ts), and the memory charts sharing the height with them.
use super::*;

/// Utilization (%) an engine has to reach within the charts' span to be charted by smart display.
const BUSY_PERCENT: f64 = 1.;
const SHARE_DEFAULT: f32 = 0.6;
const SHARE_MIN: f32 = 0.3;
const SHARE_MAX: f32 = 0.85;
/// Height of the handle between the engine and memory charts.
const HANDLE: f32 = 18.;
const LAYOUTS: [&str; 3] = ["grid", "focus", "rows"];
/// Where the engine grid keeps the size it last had, among the grid bounds.
const ENGINE_GRID: &str = "gpu-engine-grid";

fn clamp_share(share: f32) -> f32 {
    if share.is_finite() {
        share.clamp(SHARE_MIN, SHARE_MAX)
    } else {
        SHARE_DEFAULT
    }
}

// ---- Engine choice ----------------------------------------------------------------

/// What the user chose for one GPU model; absent until they choose.
#[derive(Clone, Default, PartialEq, Debug)]
pub(super) struct EngineChoice {
    /// Charts the engines that have been busy instead of a count.
    smart: bool,
    /// How many charts; none for one per engine.
    count: Option<usize>,
    /// The engines in the order the user arranged them.
    order: Vec<String>,
}
impl EngineChoice {
    fn from_json(value: &Value) -> Option<Self> {
        value.is_object().then(|| Self {
            smart: value["smart"].as_bool() == Some(true),
            count: value["count"].as_u64().map(|c| c as usize),
            order: values(&value["order"])
                .iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect(),
        })
    }
    fn to_json(&self) -> Value {
        json!({"smart": self.smart, "count": self.count, "order": self.order})
    }
}
/// Every engine, in display order: the arranged ones first, then the rest as the GPU lists them.
fn arrange(engines: &[String], choice: Option<&EngineChoice>) -> Vec<String> {
    let mut first = Vec::new();
    for engine in choice.map(|c| c.order.as_slice()).unwrap_or_default() {
        if engines.contains(engine) && !first.contains(engine) {
            first.push(engine.clone());
        }
    }
    let rest = engines
        .iter()
        .filter(|engine| !first.contains(engine))
        .cloned()
        .collect::<Vec<_>>();
    first.extend(rest);
    first
}
fn chart_count(engines: &[String], choice: Option<&EngineChoice>) -> usize {
    match choice.and_then(|c| c.count) {
        Some(count) => count.clamp(1, engines.len().max(1)),
        None => engines.len(),
    }
}
fn is_smart(choice: Option<&EngineChoice>) -> bool {
    choice.is_none_or(|c| c.smart)
}
/// The engines charted, in order: under smart display the busy ones, or the first engine.
pub(super) fn shown_engines(
    engines: &[String],
    choice: Option<&EngineChoice>,
    busy: impl Fn(&str) -> bool,
) -> Vec<String> {
    let order = arrange(engines, choice);
    if is_smart(choice) {
        let active = order
            .iter()
            .filter(|e| busy(e))
            .cloned()
            .collect::<Vec<_>>();
        return if active.is_empty() {
            order.into_iter().take(1).collect()
        } else {
            active
        };
    }
    order
        .into_iter()
        .take(chart_count(engines, choice))
        .collect()
}
fn with_smart(engines: &[String], choice: Option<&EngineChoice>) -> EngineChoice {
    EngineChoice {
        smart: true,
        count: choice.and_then(|c| c.count),
        order: arrange(engines, choice),
    }
}
/// What smart display shows, as a choice of that many charts: a manual change starts from the screen.
fn from_screen(
    engines: &[String],
    choice: Option<&EngineChoice>,
    shown: &[String],
) -> Option<EngineChoice> {
    if !is_smart(choice) {
        return choice.cloned();
    }
    let first = shown
        .iter()
        .filter(|e| engines.contains(e))
        .cloned()
        .collect::<Vec<_>>();
    let rest = arrange(engines, choice)
        .into_iter()
        .filter(|e| !first.contains(e))
        .collect::<Vec<_>>();
    let count = (first.len() != engines.len()).then_some(first.len());
    let mut order = first;
    order.extend(rest);
    Some(EngineChoice {
        smart: false,
        count,
        order,
    })
}
fn with_count(
    engines: &[String],
    choice: Option<&EngineChoice>,
    count: usize,
    shown: &[String],
) -> EngineChoice {
    let all = engines.len().max(1);
    let count = count.clamp(1, all);
    EngineChoice {
        smart: false,
        count: (count != all).then_some(count),
        order: arrange(engines, from_screen(engines, choice, shown).as_ref()),
    }
}
/// Charts `engine` in place `slot`: one charted elsewhere trades places; one not charted
/// takes the place, and the one it replaces moves just past the charted ones.
fn with_engine(
    engines: &[String],
    current: Option<&EngineChoice>,
    slot: usize,
    engine: &str,
    on_screen: &[String],
) -> EngineChoice {
    let choice = from_screen(engines, current, on_screen);
    let mut order = arrange(engines, choice.as_ref());
    let count = choice.as_ref().and_then(|c| c.count);
    let Some(from) = order.iter().position(|e| e == engine) else {
        return EngineChoice {
            smart: false,
            count,
            order,
        };
    };
    if slot >= order.len() || from == slot {
        return EngineChoice {
            smart: false,
            count,
            order,
        };
    }
    let shown = chart_count(engines, choice.as_ref());
    if from < shown {
        order.swap(slot, from);
    } else {
        let replaced = std::mem::replace(&mut order[slot], engine.to_owned());
        if let Some(duplicate) = order
            .iter()
            .enumerate()
            .skip(slot + 1)
            .find(|(_, e)| *e == engine)
            .map(|(i, _)| i)
        {
            order.remove(duplicate);
        }
        order.insert(shown.min(order.len()), replaced);
    }
    EngineChoice {
        smart: false,
        count,
        order,
    }
}

// ---- Grid plan -------------------------------------------------------------------

/// Where a chart goes: its first track and how many it spans (grid lines from 1).
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) struct Placement {
    column: usize,
    columns: usize,
    row: usize,
    rows: usize,
}
#[derive(Clone, PartialEq, Debug)]
pub(super) struct Plan {
    columns: usize,
    rows: usize,
    cells: Vec<Placement>,
    stack: usize,
}
#[derive(Clone, Copy)]
struct Shape {
    columns: usize,
    rows: usize,
    first: (usize, usize),
}
/// Width to height a small chart reads best at.
const CELL_ASPECT: f32 = 1.5;
/// How much a grid that gives the first chart the spare units counts against one that needs none.
const SPARE_WEIGHT: f32 = 0.8;
const SPARE_MAX: usize = 3;
/// Focus arrangements per count: columns, rows, the first chart's columns and rows, for a wide area.
const FOCUS: [&[[usize; 4]]; 16] = [
    &[[1, 1, 1, 1]],
    &[[2, 1, 1, 1]],
    &[[3, 2, 2, 2]],
    &[[4, 3, 3, 3]],
    &[[4, 2, 2, 2]],
    &[[3, 3, 2, 2]],
    &[[4, 3, 3, 2], [5, 2, 2, 2]],
    &[[4, 4, 3, 3], [5, 3, 4, 2]],
    &[[5, 4, 4, 3], [4, 3, 2, 2], [6, 2, 2, 2]],
    &[[5, 5, 4, 4], [6, 3, 3, 3], [5, 3, 3, 2]],
    &[[6, 5, 5, 4], [4, 4, 3, 2], [7, 2, 2, 2]],
    &[[5, 4, 3, 3], [5, 3, 2, 2]],
    &[[4, 4, 2, 2], [7, 3, 3, 3]],
    &[[5, 5, 4, 3], [7, 3, 4, 2]],
    &[[6, 5, 4, 4], [5, 4, 3, 2], [6, 3, 2, 2]],
    &[[6, 4, 3, 3], [7, 3, 3, 2]],
];
fn turned(shape: Shape) -> Shape {
    Shape {
        columns: shape.rows,
        rows: shape.columns,
        first: (shape.first.1, shape.first.0),
    }
}
/// The first chart at the top left, every other one unit, row by row.
fn place(shape: Shape, count: usize) -> Plan {
    let mut cells = vec![Placement {
        column: 1,
        columns: shape.first.0,
        row: 1,
        rows: shape.first.1,
    }];
    for row in 1..=shape.rows {
        for column in 1..=shape.columns {
            if row <= shape.first.1 && column <= shape.first.0 {
                continue;
            }
            if cells.len() < count {
                cells.push(Placement {
                    column,
                    columns: 1,
                    row,
                    rows: 1,
                });
            }
        }
    }
    Plan {
        columns: shape.columns,
        rows: shape.rows,
        cells,
        stack: shape.rows,
    }
}
/// One unit's readable size in the area, and whether it is as large as a cell may be.
fn unit(shape: Shape, width: f32, height: f32, cell: Size<Pixels>, gap: f32) -> (bool, f32) {
    let w = (width - (shape.columns - 1) as f32 * gap) / shape.columns as f32;
    let h = (height - (shape.rows - 1) as f32 * gap) / shape.rows as f32;
    let fits = w >= f32::from(cell.width) - 0.5 && h >= f32::from(cell.height) - 0.5;
    // Below the minimum the grid grows and the view scrolls: counted at half.
    (fits, (w / CELL_ASPECT).min(h) * if fits { 1. } else { 0.5 })
}
fn grid_shapes(count: usize) -> Vec<(Shape, usize)> {
    let mut shapes = Vec::new();
    for rows in 1..=count {
        let columns = count.div_ceil(rows);
        let spare = columns * rows - count;
        if spare > SPARE_MAX || (rows > 1 && count.div_ceil(rows - 1) == columns) {
            continue;
        }
        let units = spare + 1;
        let mut first: Option<(usize, usize)> = None;
        for w in (1..=units).rev() {
            if !units.is_multiple_of(w) {
                continue;
            }
            let h = units / w;
            if w > columns || h > rows || h > w {
                continue;
            }
            if first.is_none_or(|(fw, fh)| w.abs_diff(h) < fw.abs_diff(fh)) {
                first = Some((w, h));
            }
        }
        if let Some(first) = first {
            shapes.push((
                Shape {
                    columns,
                    rows,
                    first,
                },
                spare,
            ));
        }
    }
    shapes
}
pub(super) fn plan_grid(
    layout: &str,
    count: usize,
    width: f32,
    height: f32,
    cell: Size<Pixels>,
    gap: f32,
) -> Plan {
    let n = count.max(1);
    let single = Shape {
        columns: 1,
        rows: 1,
        first: (1, 1),
    };
    if width <= 0. || n == 1 {
        return place(single, n);
    }
    if layout == "rows" {
        return place(
            Shape {
                columns: 1,
                rows: n,
                first: (1, 1),
            },
            n,
        );
    }
    if layout == "focus"
        && let Some(presets) = FOCUS.get(n - 1)
    {
        let mut best: Option<(Shape, f32)> = None;
        for preset in presets.iter() {
            let wide = Shape {
                columns: preset[0],
                rows: preset[1],
                first: (preset[2], preset[3]),
            };
            let options =
                [wide, turned(wide)].map(|shape| (shape, unit(shape, width, height, cell, gap)));
            if let Some((shape, _)) = options
                .iter()
                .filter(|(_, (fits, _))| *fits)
                .max_by(|a, b| a.1.1.total_cmp(&b.1.1))
            {
                return place(*shape, n);
            }
            for (shape, (_, size)) in options {
                if best.is_none_or(|(_, s)| size > s) {
                    best = Some((shape, size));
                }
            }
        }
        if let Some((shape, _)) = best {
            return place(shape, n);
        }
    }
    let mut best: Option<(Shape, f32)> = None;
    for (shape, spare) in grid_shapes(n) {
        let score =
            unit(shape, width, height, cell, gap).1 * if spare > 0 { SPARE_WEIGHT } else { 1. };
        if best.is_none_or(|(_, s)| score > s) {
            best = Some((shape, score));
        }
    }
    place(
        best.map(|(shape, _)| shape).unwrap_or(Shape {
            columns: 1,
            rows: n,
            first: (1, 1),
        }),
        n,
    )
}

/// Drag payload for the handle between the engine and memory charts.
#[derive(Clone)]
struct ShareDrag;
struct DragGhost;
impl Render for DragGhost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

impl Desktop {
    fn engine_choices(&self) -> Value {
        self.prefs.get("gpuEngines", json!({}))
    }
    pub(super) fn gpu_view(
        &self,
        device: &Device,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let p = self.palette;
        let gpu = &self.snapshot["hardware"]["gpus"][device.index];
        let info = &gpu["info"];
        let prefix = format!("hardware.gpus.{}", device.index);
        let engines = values(&info["engines"])
            .iter()
            .filter_map(Value::as_str)
            .map(String::from)
            .collect::<Vec<_>>();
        let model_name = text(info, "name").to_owned();
        let choices = self.engine_choices();
        let choice = EngineChoice::from_json(&choices[&model_name]);
        let busy = |name: &str| {
            engines
                .iter()
                .position(|e| e == name)
                .and_then(|index| {
                    self.timeline.samples.get(&model::series_key(
                        &self.snapshot,
                        &format!("{prefix}.engines.{index}"),
                    ))
                })
                .is_some_and(|samples| {
                    samples
                        .iter()
                        .any(|(_, v)| v.is_some_and(|v| v >= BUSY_PERCENT))
                })
        };
        let shown = shown_engines(&engines, choice.as_ref(), busy);
        let layout = self.prefs.get::<String>("gpuLayout", "grid".into());
        let layout = if LAYOUTS.contains(&layout.as_str()) {
            layout
        } else {
            "grid".into()
        };
        let share_state = window.use_keyed_state("gpu-engine-share", cx, |_, _| None::<f32>);
        let share = clamp_share(
            share_state
                .read(cx)
                .unwrap_or_else(|| self.prefs.get("gpuEngineShare", SHARE_DEFAULT)),
        );
        let save = |next: EngineChoice| {
            let mut map = choices.clone();
            if !map.is_object() {
                map = json!({});
            }
            map[&model_name] = next.to_json();
            Action::Preference("gpuEngines".into(), map)
        };
        let mut out = vec![self.perf_head(
            format!("{} {}", label("common.resource.gpu"), device.index),
            Some(model_name.clone()),
            None,
            cx,
        )];
        if engines.len() > 1 {
            // A drop-down like every other choice of the original (its
            // `Select`): the list opens under the button, not at the pointer.
            let smart = is_smart(choice.as_ref());
            let mut options = vec![label("performance.gpu.engineSmart")];
            let mut actions = vec![save(with_smart(&engines, choice.as_ref()))];
            for count in 1..=engines.len() {
                options.push(if count == engines.len() {
                    tr!("performance.gpu.engineCountAll", count = count)
                } else {
                    tr!("performance.gpu.engineCountOption", count = count)
                });
                actions.push(save(with_count(&engines, choice.as_ref(), count, &shown)));
            }
            let selected = if smart { 0 } else { shown.len() };
            let select = self.select(
                "gpu-engine-count",
                label("performance.gpu.engineCount"),
                options,
                Some(selected),
                false,
                move |this, index, window, cx| {
                    if let Some(action) = actions.get(index) {
                        this.act(action.clone(), window, cx);
                    }
                },
                cx,
            );
            let select = if smart {
                div()
                    .id("gpu-engine-count-tip")
                    .flex_shrink_0()
                    .child(select)
                    .tooltip(|window, cx| {
                        widgets::tooltip(label("performance.gpu.engineSmartTitle"), window, cx)
                    })
                    .into_any_element()
            } else {
                select
            };
            let layouts = self.segments(
                "gpu-layout",
                LAYOUTS
                    .into_iter()
                    .map(|choice| {
                        (
                            label(&format!("performance.gpu.layouts.{choice}")),
                            None,
                            Action::Preference("gpuLayout".into(), json!(choice)),
                            layout == choice,
                        )
                    })
                    .collect(),
                cx,
            );
            out.push(self.perf_toolbar(vec![select, layouts]));
        }
        out.push(
            div()
                .flex()
                .flex_shrink_0()
                .justify_between()
                .my(px(2.))
                .text_size(theme::font(12.))
                .text_color(p.muted)
                .child(label("performance.gpu.engineUtilization"))
                .child("100%")
                .into_any_element(),
        );

        // Engine charts, each in the place the plan gives it.
        let gap = theme::dimension("coreGap");
        let cell = size(
            theme::dimension("engineMinWidth"),
            theme::dimension("engineMinHeight"),
        );
        let header = theme::dimension("coreHeaderHeight");
        // The grid's size in this display; until it is known, the size it last had.
        let measured_state =
            window.use_keyed_state("gpu-engine-grid-size", cx, |_, _| None::<Size<Pixels>>);
        let here = *measured_state.read(cx);
        let measured = here.unwrap_or_else(|| {
            self.grid_bounds
                .borrow()
                .get(ENGINE_GRID)
                .map_or_else(Size::default, |bounds| bounds.size)
        });
        let policy = super::settle_policy("gpu-engine-settled", here.is_some(), window, cx);
        let width = if measured.width > px(0.) {
            f32::from(measured.width)
        } else {
            self.detail_width(window)
        };
        let plan = plan_grid(
            &layout,
            shown.len(),
            width,
            f32::from(measured.height),
            cell,
            f32::from(gap),
        );
        let track_width = (px(width) - gap * (plan.columns - 1) as f32) / plan.columns as f32;
        let track_height =
            ((measured.height - gap * (plan.rows - 1) as f32) / plan.rows as f32).max(cell.height);
        let owner = cx.entity().downgrade();
        let mut grid = div()
            .relative()
            .w_full()
            .flex_grow(share)
            .flex_shrink(1.)
            .flex_basis(px(0.))
            .min_h((cell.height + gap) * plan.stack as f32 - gap)
            .child(
                canvas(
                    move |bounds, window, cx| {
                        let changed = measured_state
                            .update(cx, |size, _| size.replace(bounds.size))
                            != Some(bounds.size);
                        if changed {
                            let owner = owner.clone();
                            window.defer(cx, move |_, cx| {
                                let _ = owner.update(cx, |_, cx| cx.notify());
                            });
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            );
        if let Some(size) = here {
            self.grid_bounds
                .borrow_mut()
                .insert(ENGINE_GRID.into(), Bounds::new(Point::default(), size));
        }
        // Never laid out before: the charts wait one frame for the real size
        // rather than appearing in a guessed arrangement.
        let place = here.is_some() || measured.height > px(0.);
        for (slot, name) in shown.iter().enumerate().filter(|_| place) {
            let Some(index) = engines.iter().position(|e| e == name) else {
                continue;
            };
            let at = plan.cells.get(slot).copied().unwrap_or(Placement {
                column: 1,
                columns: 1,
                row: 1,
                rows: 1,
            });
            let target = Bounds::new(
                point(
                    (track_width + gap) * (at.column - 1) as f32,
                    (track_height + gap) * (at.row - 1) as f32,
                ),
                size(
                    track_width * at.columns as f32 + gap * (at.columns - 1) as f32,
                    track_height * at.rows as f32 + gap * (at.rows - 1) as f32,
                ),
            );
            let bounds = gpui_kit::base::motion::transition(
                SharedString::from(format!("gpu-engine-bounds-{name}")),
                target,
                policy.clone(),
                window,
                cx,
            );
            let chart = self.chart_view(
                &format!("gpu-engine-{index}"),
                &[Line::new(format!("{prefix}.engines.{index}"), p.gpu)],
                Some(Scale::Fixed(100.)),
                (bounds.size.height - header).max(px(1.)),
                Look {
                    frame: true,
                    ..Look::default()
                },
                cx,
            );
            let actions = engines
                .iter()
                .map(|engine| save(with_engine(&engines, choice.as_ref(), slot, engine, &shown)))
                .collect::<Vec<_>>();
            let load = gpu["engines"][index].as_f64().unwrap_or(0.);
            let head = div()
                .h(header)
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .px(px(1.))
                .pb(px(3.))
                .whitespace_nowrap()
                .child(
                    div()
                        .min_w_0()
                        .flex()
                        .text_size(theme::font(12.))
                        .font_weight(FontWeight::MEDIUM)
                        .child(self.select_compact(
                            format!("gpu-engine-choice-{slot}"),
                            label("performance.gpu.engineType"),
                            engines.clone(),
                            engines.iter().position(|engine| engine == name),
                            move |this, index, window, cx| {
                                if let Some(action) = actions.get(index) {
                                    this.act(action.clone(), window, cx);
                                }
                            },
                            cx,
                        )),
                )
                .child(
                    div()
                        .flex_none()
                        .flex()
                        .items_baseline()
                        .text_size(theme::font(13.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(format!("{}", load.round()))
                        .child(
                            div()
                                .ml(px(1.))
                                .text_size(theme::font(10.5))
                                .font_weight(FontWeight::NORMAL)
                                .text_color(p.muted)
                                .child("%"),
                        ),
                );
            grid = grid.child(
                div()
                    .absolute()
                    .left(bounds.origin.x)
                    .top(bounds.origin.y)
                    .w(bounds.size.width)
                    .h(bounds.size.height)
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .child(head)
                    .child(chart),
            );
        }
        let handle_state = share_state.clone();
        let handle = div()
            .id("gpu-share-splitter")
            .group("gpu-share")
            .relative()
            .flex_none()
            .h(px(HANDLE))
            .cursor_row_resize()
            .child(
                div()
                    .absolute()
                    .top(px(8.))
                    .left(relative(0.5))
                    .ml(px(-20.))
                    .w(px(40.))
                    .h(px(2.))
                    .rounded(px(1.))
                    .bg(p.stroke_strong)
                    .opacity(0.45)
                    .group_hover("gpu-share", |s| s.opacity(1.)),
            )
            .on_drag(ShareDrag, |_, _, _, cx| cx.new(|_| DragGhost))
            .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                if event.click_count() >= 2 {
                    handle_state.update(cx, |share, _| *share = Some(SHARE_DEFAULT));
                    this.prefs.set("gpuEngineShare", SHARE_DEFAULT);
                    cx.notify();
                }
            }));
        let mut memory_chart = |id: &str, key: &str, caption: &str, total: f64| {
            self.chart_view(
                id,
                &[Line::new(format!("{prefix}.{key}"), p.gpu)],
                Some(Scale::Fixed(if total > 0. { total } else { 1. })),
                ChartHeight::Fill,
                Look::boxed(label(caption))
                    .top(format!("{} GB", gb(total)))
                    .format(MetricFormat::Gigabytes)
                    .min_height(px(SMALL_MIN)),
                cx,
            )
        };
        let pair = div()
            .grid()
            .grid_cols(2)
            .gap(px(16.))
            .flex_grow(1. - share)
            .flex_shrink(1.)
            .flex_basis(px(0.))
            .child(div().flex().flex_col().child(memory_chart(
                "gpu-memory-dedicated",
                "dedicatedUsed",
                "performance.gpu.dedicatedMemory",
                number(info, "dedicatedTotal"),
            )))
            .child(div().flex().flex_col().child(memory_chart(
                "gpu-memory-shared",
                "sharedUsed",
                "performance.gpu.sharedMemory",
                number(info, "sharedTotal"),
            )));
        let move_state = share_state.clone();
        let drop_state = share_state;
        out.push(
            div()
                .id("gpu-charts")
                .flex_1()
                .flex()
                .flex_col()
                .child(grid)
                .child(handle)
                .child(pair)
                .on_drag_move(
                    cx.listener(move |_, event: &DragMoveEvent<ShareDrag>, _, cx| {
                        let bounds = event.bounds;
                        let usable = f32::from(bounds.size.height) - HANDLE;
                        if usable <= 0. {
                            return;
                        }
                        let next = clamp_share(
                            (f32::from(event.event.position.y - bounds.top()) - HANDLE / 2.)
                                / usable,
                        );
                        move_state.update(cx, |share, _| *share = Some(next));
                        cx.notify();
                    }),
                )
                .on_drop(cx.listener(move |this, _: &ShareDrag, _, cx| {
                    if let Some(share) = *drop_state.read(cx) {
                        this.prefs.set("gpuEngineShare", share);
                    }
                    cx.notify();
                }))
                .into_any_element(),
        );

        // Live figures and specs. Zero is a valid clock, voltage or power reading;
        // only a missing one is left out.
        let sensors = &gpu["integratedSensors"];
        let integrated = boolean(info, "intelIntegrated");
        let reading = |value: &Value, key: &str| value[key].as_f64();
        let gb_pair = |used: f64, total: f64| format!("{} / {}", gb(used), gb(total));
        let source = gpu["utilSource"].as_str().unwrap_or("windows");
        let mut stats = vec![
            self.hinted(
                "gpu-stat-util",
                Some(label("performance.gpu.utilizationHint")),
                self.stat(
                    label("performance.gpu.utilization"),
                    format!("{}", number(gpu, "util").round()),
                    Some("%".into()),
                    Some(label(&format!("performance.gpu.utilSource.{source}"))),
                    cx,
                ),
            ),
            self.stat(
                label("performance.gpu.dedicatedMemory"),
                gb_pair(number(gpu, "dedicatedUsed"), number(info, "dedicatedTotal")),
                Some("GB".into()),
                None,
                cx,
            ),
            self.stat(
                label("performance.gpu.sharedMemory"),
                gb_pair(number(gpu, "sharedUsed"), number(info, "sharedTotal")),
                Some("GB".into()),
                None,
                cx,
            ),
        ];
        if let Some(celsius) = reading(gpu, "temperature") {
            stats.push(self.stat(
                label("performance.gpu.temperature"),
                format!("{}", celsius.round()),
                Some("°C".into()),
                None,
                cx,
            ));
        }
        if let Some(rpm) = reading(gpu, "fanRpm") {
            stats.push(self.stat(
                label("performance.gpu.fan"),
                theme::count(rpm),
                Some("RPM".into()),
                None,
                cx,
            ));
        }
        // Figures with an optional explanation: (id, title, value, unit, hint).
        let mut figures: Vec<(&str, &str, String, &str, Option<&str>)> = Vec::new();
        if let Some(mhz) = reading(gpu, "clockMhz") {
            figures.push((
                "gpu-stat-clock",
                "performance.gpu.coreClock",
                clock(mhz),
                "MHz",
                (!sensors.is_null()).then_some("performance.gpu.clockHint"),
            ));
        }
        if let Some(mhz) = reading(gpu, "memoryClockMhz") {
            figures.push(if integrated {
                (
                    "gpu-stat-memory-clock",
                    "performance.gpu.sharedMemoryClock",
                    clock(mhz),
                    "MHz",
                    Some("performance.gpu.sharedMemoryClockHint"),
                )
            } else {
                (
                    "gpu-stat-memory-clock",
                    "performance.gpu.memoryClock",
                    clock(mhz),
                    "MHz",
                    None,
                )
            });
        }
        if let Some(mhz) = reading(sensors, "unsliceClockMhz") {
            figures.push((
                "gpu-stat-unslice-clock",
                "performance.gpu.unsliceClock",
                clock(mhz),
                "MHz",
                Some("performance.gpu.unsliceClockHint"),
            ));
        }
        if let Some(millivolts) = reading(gpu, "voltageMv") {
            figures.push((
                "gpu-stat-voltage",
                "performance.gpu.coreVoltage",
                format!("{:.3}", millivolts / 1000.),
                "V",
                None,
            ));
        }
        if let Some(millivolts) = reading(gpu, "coreVidMv") {
            figures.push((
                "gpu-stat-vid",
                "performance.gpu.coreVid",
                format!("{:.3}", millivolts / 1000.),
                "V",
                Some("performance.gpu.vidHint"),
            ));
        }
        for (field, id, title) in [
            (
                "gtActivityPercent",
                "gpu-stat-gt-activity",
                "performance.gpu.gtActivity",
            ),
            (
                "mediaActivityPercent",
                "gpu-stat-media-activity",
                "performance.gpu.mediaActivity",
            ),
        ] {
            if let Some(percent) = reading(sensors, field) {
                figures.push((
                    id,
                    title,
                    format!("{percent:.1}"),
                    "%",
                    Some("performance.gpu.activityHint"),
                ));
            }
        }
        for (id, title, value, unit, hint) in figures {
            let stat = self.stat(label(title), value, Some(unit.into()), None, cx);
            stats.push(self.hinted(id, hint.map(label), stat));
        }
        if let Some(watts) = reading(gpu, "boardPower") {
            stats.push(self.stat(
                label("performance.gpu.power"),
                charts::watts(watts),
                Some("W".into()),
                None,
                cx,
            ));
        } else if let Some(percent) = reading(gpu, "power").filter(|v| *v != 0.) {
            stats.push(self.stat(
                label("performance.gpu.power"),
                format!("{}", percent.round()),
                Some("% TDP".into()),
                None,
                cx,
            ));
        }
        let plain = |value: String| div().child(value).into_any_element();
        let mut rows = Vec::new();
        if let Some(source) = gpu["powerSource"].as_str() {
            rows.push((
                label("performance.gpu.powerSource"),
                plain(label(&format!("performance.gpu.sources.{source}"))),
            ));
        }
        let kernel_supported = boolean(gpu, "kernelSensorSupported");
        let kernel_state = gpu["kernelSensorState"].as_str();
        if kernel_supported {
            rows.push((
                label("performance.gpu.kernelSensors"),
                plain(match kernel_state {
                    Some(state) => label(&format!("performance.cpu.sensorState.{state}")),
                    None => label("performance.cpu.notEnabled"),
                }),
            ));
        }
        rows.push((
            label("performance.gpu.dedicatedMemory"),
            plain(format!("{} GB", gb(number(info, "dedicatedTotal")))),
        ));
        rows.push((
            label("performance.gpu.sharedMemory"),
            plain(format!("{} GB", gb(number(info, "sharedTotal")))),
        ));
        if let Some(limit) = reading(gpu, "boardPowerLimit").filter(|v| *v != 0.) {
            rows.push((
                label("performance.gpu.powerLimit"),
                plain(format!("{} W", charts::watts(limit))),
            ));
        }
        if let Some(max) = reading(info, "temperatureMax").filter(|v| *v != 0.) {
            rows.push((
                label("performance.gpu.maxTemperature"),
                plain(format!("{} °C", max.round())),
            ));
        }
        rows.push((
            label("performance.gpu.engines"),
            plain(engines.len().to_string()),
        ));
        if let Some(max) = reading(gpu, "clockMaxMhz").filter(|v| *v != 0.) {
            rows.push((
                label("performance.gpu.maxCoreClock"),
                plain(format!("{} MHz", clock(max))),
            ));
        }
        if let Some(min) = reading(sensors, "minimumClockMhz") {
            rows.push((
                label("performance.gpu.minCoreClock"),
                plain(format!("{} MHz", clock(min))),
            ));
        }
        if let Some(reference) = reading(sensors, "referenceClockMhz") {
            rows.push((
                label("performance.gpu.referenceClock"),
                plain(format!("{} MHz", decimal(reference, 3))),
            ));
        }
        let limits = values(&sensors["limits"]);
        if !limits.is_empty() {
            let reason = |limit: &Value| {
                label(&format!(
                    "performance.gpu.limitReasons.{}",
                    text(limit, "reason")
                ))
            };
            let active = limits.iter().filter(|l| boolean(l, "active")).count();
            let summary = if active > 0 {
                tr!("performance.gpu.limitsActiveCount", count = active)
            } else {
                label("performance.gpu.noLimit")
            };
            // Each reason, whether it limits now, and whether it did since the last read.
            let details = limits
                .iter()
                .map(|limit| {
                    let state = label(if boolean(limit, "active") {
                        "performance.gpu.limitActive"
                    } else {
                        "performance.gpu.noLimit"
                    });
                    let occurred = if boolean(limit, "occurred") {
                        format!(" ({})", label("performance.gpu.limitOccurred"))
                    } else {
                        String::new()
                    };
                    format!("{}: {state}{occurred}", reason(limit))
                })
                .collect::<Vec<_>>()
                .join("\n");
            rows.push((
                label("performance.gpu.performanceLimits"),
                div()
                    .id("gpu-limits")
                    .child(summary)
                    .tooltip(move |window, cx| widgets::tooltip(details.clone(), window, cx))
                    .into_any_element(),
            ));
        }
        out.push(self.perf_stats(stats, self.info_list(rows), false, window));
        if kernel_supported && kernel_state != Some("ready") {
            out.push(self.perf_note(label(if kernel_state.is_some() {
                "performance.gpu.kernelUnavailable"
            } else {
                "performance.gpu.kernelEnable"
            })));
        }
        out
    }
}

/// A clock to a tenth of a MHz, grouped like the other counts: 1,950, 433.3.
fn clock(mhz: f64) -> String {
    decimal((mhz * 10.).round() / 10., 1)
}
/// A number grouped by thousands with up to `digits` decimals, trailing zeros
/// dropped, as `Intl.NumberFormat` shows it: 1,972.5, 24, 19.2.
fn decimal(value: f64, digits: usize) -> String {
    let fixed = format!("{:.*}", digits, value.abs());
    let (whole, fraction) = fixed.split_once('.').unwrap_or((&fixed, ""));
    let fraction = fraction.trim_end_matches('0');
    let mut out = String::new();
    if value < 0. && fixed.bytes().any(|b| b.is_ascii_digit() && b != b'0') {
        out.push('-');
    }
    for (i, c) in whole.chars().enumerate() {
        if i > 0 && (whole.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    if !fraction.is_empty() {
        out.push('.');
        out.push_str(fraction);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{LAYOUTS, plan_grid, shown_engines, with_count, with_engine};
    use gpui_kit::gpui::{px, size};
    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }
    #[test]
    fn smart_display_charts_busy_engines_or_the_first() {
        let engines = names(&["3D", "Copy", "Video Decode"]);
        assert_eq!(
            shown_engines(&engines, None, |e| e == "Copy"),
            names(&["Copy"])
        );
        assert_eq!(shown_engines(&engines, None, |_| false), names(&["3D"]));
    }
    #[test]
    fn choosing_an_engine_swaps_or_replaces_a_slot() {
        let engines = names(&["3D", "Copy", "Video Decode", "Compute"]);
        let two = with_count(&engines, None, 2, &names(&["3D", "Copy"]));
        assert_eq!(two.count, Some(2));
        let swapped = with_engine(&engines, Some(&two), 0, "Copy", &[]);
        assert_eq!(&swapped.order[..2], &names(&["Copy", "3D"])[..]);
        let replaced = with_engine(&engines, Some(&two), 1, "Compute", &[]);
        assert_eq!(&replaced.order[..3], &names(&["3D", "Compute", "Copy"])[..]);
    }
    #[test]
    fn plans_fill_their_grid() {
        let cell = size(px(150.), px(76.));
        for layout in LAYOUTS {
            for count in 1..=12 {
                let plan = plan_grid(layout, count, 900., 400., cell, 6.);
                let mut seen = std::collections::HashSet::new();
                for c in &plan.cells {
                    for x in c.column..c.column + c.columns {
                        for y in c.row..c.row + c.rows {
                            assert!(seen.insert((x, y)), "{layout} {count} overlaps");
                            assert!(x <= plan.columns && y <= plan.rows);
                        }
                    }
                }
                assert_eq!(plan.cells.len(), count);
                if layout != "rows" {
                    assert_eq!(seen.len(), plan.columns * plan.rows, "{layout} {count}");
                }
            }
        }
    }
}
