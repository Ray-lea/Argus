//! Argus patch: a cached view drawn again only because isolated views lie
//! inside it replays its last frame around the cached views that hold them,
//! each drawn again in its place, and the cached views inside a view that
//! replays move their ranges along with it.
use super::*;
use crate::{DispatchNodeId, StretchReuse};

/// Argus patch: a cached view drawn again in place in a view that replays
/// around it, to paint where its last paint lay in the replayed view's.
pub(super) struct SplicedView {
    entity_id: EntityId,
    name: &'static str,
    global_id: GlobalElementId,
    previous_paint: Range<PaintIndex>,
    /// The copy of its dispatch node.
    node: Option<DispatchNodeId>,
    element_id_stack: ElementIdStack,
    text_style_stack: TextStyleStack,
    rendered_entity_stack: Vec<EntityId>,
    rem_size: Pixels,
    paint: SplicePaint,
    prepaint: Option<ViewPrepaint>,
}

/// Argus patch: a cached view of the last frame to draw again in place.
pub(super) struct Hole {
    entity_id: EntityId,
    name: &'static str,
    global_id: GlobalElementId,
    view: AnyView,
    weak_view: AnyWeakView,
    bounds: Bounds<Pixels>,
    content_mask: ContentMask<Pixels>,
    opacity: f32,
    element_id_stack: ElementIdStack,
    text_style_stack: TextStyleStack,
    rendered_entity_stack: Vec<EntityId>,
    rem_size: Pixels,
    paint: SplicePaint,
    prepaint_range: Range<PrepaintStateIndex>,
    paint_range: Range<PaintIndex>,
}

/// Argus patch: the cached views to draw again in their places while `view`,
/// which is drawn again only because isolated views lie inside it
/// (`Window::request_isolated_animation_frame`), replays its last frame
/// around them: for each such isolated view, the outermost cached view
/// between `view` and it, or the isolated view itself if it is cached.
/// `None` if `view` must render instead: it is dirty for itself or for a
/// view inside it that is not isolated, an isolated view inside it has no
/// cached view around it below `view`, or such a cached view's last frame
/// cannot be told apart from the rest of `view`'s.
pub(super) fn splice_holes(
    window: &Window,
    view: EntityId,
    state: &ViewElementState,
    generation: u64,
) -> Option<SmallVec<[Hole; 2]>> {
    if !window.isolated_ancestors.contains(&view)
        || window.isolated_views.contains(&view)
        || window.a11y.is_active()
    {
        return None;
    }
    let frame = &window.rendered_frame;
    let mut holes = SmallVec::<[Hole; 2]>::new();
    for &isolated in &window.isolated_views {
        let mut outermost = None;
        let mut inside = false;
        for ancestor in frame.dispatch_tree.view_path_reversed(isolated) {
            if ancestor == view {
                inside = true;
                break;
            }
            if frame.cached_views.contains_key(&ancestor) {
                outermost = Some(ancestor);
            }
        }
        if !inside {
            continue;
        }
        let hole = outermost?;
        if holes.iter().any(|known| known.entity_id == hole) {
            continue;
        }
        holes.push(hole_in(window, hole, state, generation)?);
    }
    if holes.is_empty() {
        return None;
    }
    holes.sort_by_key(|hole| hole.prepaint_range.start.dispatch_nodes());
    // In order, apart, and each after the node drawn for the next one: the
    // stretches between them are what replays.
    let ordered = holes.windows(2).all(|pair| {
        let (before, after) = (&pair[0], &pair[1]);
        before
            .prepaint_range
            .end
            .not_after(&after.prepaint_range.start.before_last_state())
            && before
                .paint_range
                .end
                .not_after(&after.paint_range.start.before_last_state())
            && before.prepaint_range.end.dispatch_nodes()
                < after.prepaint_range.start.dispatch_nodes()
    });
    ordered.then_some(holes)
}

/// Argus patch: `view`, a cached view drawn in the last frame inside the
/// view whose state is `outer`, as a view to draw again in place there.
fn hole_in(
    window: &Window,
    view: EntityId,
    outer: &ViewElementState,
    generation: u64,
) -> Option<Hole> {
    let global_id = window.rendered_frame.cached_views.get(&view)?;
    let state = window.rendered_element_state::<ViewElementState>(global_id)?;
    let splice = state.splice.as_ref()?;
    let paint = splice.paint.clone()?;
    let last = generation.wrapping_sub(1);
    let fits = state.view == view
        && state.prepainted == last
        && state.painted == last
        && outer
            .prepaint_range
            .start
            .not_after(&state.prepaint_range.start.before_last_state())
        && state
            .prepaint_range
            .end
            .not_after(&outer.prepaint_range.end)
        && outer
            .paint_range
            .start
            .not_after(&state.paint_range.start.before_last_state())
        && state.paint_range.end.not_after(&outer.paint_range.end)
        // Its own state was used just before each of its ranges: the
        // stretch before it stops short of that use, which drawing it again
        // repeats (copied as well, the use would pile up frame after frame).
        && window.rendered_state_used_before::<ViewElementState>(
            state.prepaint_range.start.element_states(),
            global_id,
        )
        && window.rendered_state_used_before::<ViewElementState>(
            state.paint_range.start.element_states(),
            global_id,
        )
        && window.splice_fits(view, &outer.prepaint_range, &state.prepaint_range);
    if !fits {
        return None;
    }
    Some(Hole {
        entity_id: view,
        name: splice.view.view_type_name,
        global_id: global_id.clone(),
        view: splice.view.upgrade()?,
        weak_view: splice.view.clone(),
        bounds: state.cache_key.bounds,
        content_mask: state.cache_key.content_mask.clone(),
        opacity: state.cache_key.opacity,
        element_id_stack: splice.element_id_stack.clone(),
        text_style_stack: splice.text_style_stack.clone(),
        rendered_entity_stack: splice.rendered_entity_stack.clone(),
        rem_size: splice.rem_size,
        paint,
        prepaint_range: state.prepaint_range.clone(),
        paint_range: state.paint_range.clone(),
    })
}

/// Argus patch: replays the prepaint of a view around the cached views in
/// `holes` (in order), each drawn again in its place, in what it was drawn
/// inside in the last frame. The stretches between them replay as a whole
/// view does (`reuse_prepaint`), and the cached views inside those move along
/// (`rebase_nested_prepaint`).
pub(super) fn splice_prepaint(
    window: &mut Window,
    cx: &mut App,
    state: &mut ViewElementState,
    holes: SmallVec<[Hole; 2]>,
    generation: u64,
) -> Vec<SplicedView> {
    let previous = state.prepaint_range.clone();
    let prepaint_start = window.prepaint_index();
    cx.entities
        .extend_accessed(state.accessed_entities.iter().copied());
    let mut reuse = StretchReuse::default();
    let mut from = previous.start.clone();
    let mut spliced = Vec::with_capacity(holes.len());
    for hole in holes {
        prepaint_stretch(
            window,
            &mut reuse,
            from..hole.prepaint_range.start.before_last_state(),
            generation,
        );
        let node = window.enter_spliced_view(&mut reuse, &hole.prepaint_range);
        debug_assert!(
            node.is_some(),
            "the node drawn for a spliced view is not open"
        );
        let context = SpliceScope {
            element_id_stack: &hole.element_id_stack,
            text_style_stack: &hole.text_style_stack,
            rendered_entity_stack: &hole.rendered_entity_stack,
            rem_size: hole.rem_size,
            content_mask: hole.content_mask.clone(),
            opacity: hole.opacity,
            hover_fills_in_place: None,
        };
        let (prepaint, accessed) = cx.detect_accessed_entities(|cx| {
            context.enter(window, |window| {
                #[cfg(feature = "profiler")]
                let start = window.window_profiler.begin_view();
                let view = hole.view.clone();
                let prepaint = prepaint_view(
                    hole.entity_id,
                    hole.name,
                    Some(&hole.weak_view),
                    // Drawn without the view around it: dirty, it renders,
                    // or replays around a view inside it in its turn.
                    false,
                    Some(&hole.global_id),
                    hole.bounds,
                    &mut None,
                    window,
                    cx,
                    &mut |window, cx| render_view(view.clone(), window, cx),
                );
                #[cfg(feature = "profiler")]
                window.window_profiler.end_view(
                    start,
                    hole.name,
                    crate::profiler::ViewPhase::Prepaint,
                );
                prepaint
            })
        });
        state.accessed_entities.extend(accessed);
        from = hole.prepaint_range.end.clone();
        spliced.push(SplicedView {
            entity_id: hole.entity_id,
            name: hole.name,
            global_id: hole.global_id,
            previous_paint: hole.paint_range,
            node,
            element_id_stack: hole.element_id_stack,
            text_style_stack: hole.text_style_stack,
            rendered_entity_stack: hole.rendered_entity_stack,
            rem_size: hole.rem_size,
            paint: hole.paint,
            prepaint,
        });
    }
    prepaint_stretch(window, &mut reuse, from..previous.end.clone(), generation);
    window.finish_prepaint_reuse(&mut reuse);
    state.prepaint_range = prepaint_start..window.prepaint_index();
    state.prepainted = generation;
    spliced
}

/// Argus patch: one stretch of a view's prepaint replayed around views drawn
/// again in place, and the cached views in it moved along.
fn prepaint_stretch(
    window: &mut Window,
    reuse: &mut StretchReuse,
    stretch: Range<PrepaintStateIndex>,
    generation: u64,
) {
    let start = window.prepaint_index();
    window.reuse_prepaint_stretch(stretch.clone(), reuse);
    rebase_nested_prepaint(window, &stretch, &start, generation);
}

/// Argus patch: the paint half of [`splice_prepaint`]: the stretches of the
/// view's last paint replay around the views drawn again, painted in place.
pub(super) fn splice_paint(
    window: &mut Window,
    cx: &mut App,
    state: &mut ViewElementState,
    spliced: &mut [SplicedView],
    generation: u64,
) {
    let previous = state.paint_range.clone();
    let mut from = previous.start.clone();
    for view in spliced.iter_mut() {
        paint_stretch(
            window,
            from..view.previous_paint.start.before_last_state(),
            generation,
        );
        if let Some(node) = view.node {
            window.activate_dispatch_node(node);
        }
        let context = SpliceScope {
            element_id_stack: &view.element_id_stack,
            text_style_stack: &view.text_style_stack,
            rendered_entity_stack: &view.rendered_entity_stack,
            rem_size: view.rem_size,
            content_mask: view.paint.content_mask.clone(),
            opacity: view.paint.opacity,
            hover_fills_in_place: Some(view.paint.hover_fills_in_place),
        };
        let (entity_id, name, global_id) = (view.entity_id, view.name, &view.global_id);
        let groups = &view.paint.groups;
        let prepaint = &mut view.prepaint;
        let ((), accessed) = cx.detect_accessed_entities(|cx| {
            context.enter(window, |window| {
                #[cfg(feature = "profiler")]
                let start = window.window_profiler.begin_view();
                // The groups set around it by elements that replayed, with the
                // hitboxes they replayed with; those painted anew this frame
                // have set theirs.
                let mut pushed = SmallVec::<[SharedString; 2]>::new();
                for (group, hitbox) in groups {
                    if GroupHitboxes::get(group, cx).is_none() {
                        GroupHitboxes::push(group.clone(), *hitbox, cx);
                        pushed.push(group.clone());
                    }
                }
                paint_view(entity_id, true, Some(global_id), prepaint, window, cx);
                for group in &pushed {
                    GroupHitboxes::pop(group, cx);
                }
                #[cfg(feature = "profiler")]
                window
                    .window_profiler
                    .end_view(start, name, crate::profiler::ViewPhase::Paint);
                #[cfg(not(feature = "profiler"))]
                let _ = name;
            })
        });
        state.accessed_entities.extend(accessed);
        from = view.previous_paint.end.clone();
    }
    paint_stretch(window, from..previous.end.clone(), generation);
}

/// Argus patch: one stretch of a view's paint replayed around views drawn
/// again in place, and the cached views in it moved along.
fn paint_stretch(window: &mut Window, stretch: Range<PaintIndex>, generation: u64) {
    let start = window.paint_index();
    window.reuse_paint(stretch.clone());
    rebase_nested_paint(window, &stretch, &start, generation);
}

/// Argus patch: what a view drawn again in place is drawn inside, as when it
/// was last rendered, in place of what the window holds at that point of the
/// replay around it (nothing of the replayed view's own tree).
struct SpliceScope<'a> {
    element_id_stack: &'a ElementIdStack,
    text_style_stack: &'a TextStyleStack,
    rendered_entity_stack: &'a [EntityId],
    rem_size: Pixels,
    content_mask: ContentMask<Pixels>,
    opacity: f32,
    /// At paint: how many elements around it let hover fills change in place.
    hover_fills_in_place: Option<usize>,
}

impl SpliceScope<'_> {
    fn enter<R>(&self, window: &mut Window, f: impl FnOnce(&mut Window) -> R) -> R {
        let element_id_stack =
            mem::replace(&mut window.element_id_stack, self.element_id_stack.clone());
        let text_style_stack =
            mem::replace(&mut window.text_style_stack, self.text_style_stack.clone());
        let rendered_entity_stack = mem::replace(
            &mut window.rendered_entity_stack,
            self.rendered_entity_stack.to_vec(),
        );
        let content_mask_stack = mem::replace(
            &mut window.content_mask_stack,
            vec![self.content_mask.clone()],
        );
        let element_opacity = mem::replace(&mut window.element_opacity, self.opacity);
        let hover_fills_in_place = self
            .hover_fills_in_place
            .map(|fills| mem::replace(&mut window.hover_fills_in_place, fills));
        let pending_hover_background = window.pending_hover_background.take();

        let result = window.with_rem_size(Some(self.rem_size), f);

        window.pending_hover_background = pending_hover_background;
        if let Some(fills) = hover_fills_in_place {
            window.hover_fills_in_place = fills;
        }
        window.element_opacity = element_opacity;
        window.content_mask_stack = content_mask_stack;
        window.rendered_entity_stack = rendered_entity_stack;
        window.text_style_stack = text_style_stack;
        window.element_id_stack = element_id_stack;
        result
    }
}

/// A view replayed from the last frame is not visited, so the cached views
/// drawn inside it would keep ranges that index the frame they were last
/// visited in, and could not be replayed on their own once the view around
/// them renders again: any frame that replayed the whole window's content (a
/// layer above it animating, say) made the next change render every view.
/// The replay copies the last frame's lists entry for entry, so what a view
/// inside recorded lies as far into the copy as it lay into the original:
/// its ranges move along. `previous` is the replayed view's range in the last
/// frame and `start` where its replay began in this one.
///
/// Each range moves on its own and takes the frame's number as it does
/// (`prepainted`, `painted`): a view replays only when both index the last
/// frame, so one whose paint did not follow its prepaint renders instead.
pub(super) fn rebase_nested_prepaint(
    window: &mut Window,
    previous: &Range<PrepaintStateIndex>,
    start: &PrepaintStateIndex,
    generation: u64,
) {
    let mut moved = SmallVec::<[(EntityId, GlobalElementId); 4]>::new();
    window.for_reused_element_states::<ViewElementState>(
        PrepaintStateIndex::accessed_element_states(previous),
        |global_id, nested| {
            // Only ranges of the last frame that lie inside the replayed one:
            // a view drawn from a deferred element lies after it.
            if nested.prepainted != generation.wrapping_sub(1)
                || nested.painted != generation.wrapping_sub(1)
                || !nested.prepaint_range.end.not_after(&previous.end)
            {
                return;
            }
            if let (Some(from), Some(to)) = (
                nested.prepaint_range.start.rebased(&previous.start, start),
                nested.prepaint_range.end.rebased(&previous.start, start),
            ) {
                nested.prepaint_range = from..to;
                nested.prepainted = generation;
                moved.push((nested.view, global_id.clone()));
            }
        },
    );
    // Argus patch: they can be drawn again in place in the next frame.
    window.next_frame.cached_views.extend(moved);
}

/// The paint half of [`rebase_nested_prepaint`], for the views whose
/// prepaint range moved in this frame: a view whose two ranges both moved is
/// as good as drawn in it.
pub(super) fn rebase_nested_paint(
    window: &mut Window,
    previous: &Range<PaintIndex>,
    start: &PaintIndex,
    generation: u64,
) {
    window.for_reused_element_states::<ViewElementState>(
        PaintIndex::accessed_element_states(previous),
        |_, nested| {
            if nested.prepainted != generation
                || nested.painted != generation.wrapping_sub(1)
                || !nested.paint_range.end.not_after(&previous.end)
            {
                return;
            }
            if let (Some(from), Some(to)) = (
                nested.paint_range.start.rebased(&previous.start, start),
                nested.paint_range.end.rebased(&previous.start, start),
            ) {
                nested.paint_range = from..to;
                nested.painted = generation;
            }
        },
    );
}
