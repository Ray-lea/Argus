//! Argus patch: cached views replaying their last frame: the indices of a
//! frame's lists moved along with a replay, the element states a replay
//! reads, and the stretches replayed around views drawn again in place.
use super::*;

/// `index`, counted inside a stretch of a list starting at `from`, where that
/// stretch was copied starting at `to`. `None` if it lies before the stretch.
fn rebase_index(index: usize, from: usize, to: usize) -> Option<usize> {
    index.checked_sub(from).map(|offset| to + offset)
}

impl PrepaintStateIndex {
    /// This index, taken inside the stretch of the last frame that starts at
    /// `from`, where a replay of that stretch starting at `to` put it: a
    /// replay copies every list it counts entry for entry.
    pub(crate) fn rebased(&self, from: &Self, to: &Self) -> Option<Self> {
        Some(Self {
            hitboxes_index: rebase_index(
                self.hitboxes_index,
                from.hitboxes_index,
                to.hitboxes_index,
            )?,
            tooltips_index: rebase_index(
                self.tooltips_index,
                from.tooltips_index,
                to.tooltips_index,
            )?,
            deferred_draws_index: rebase_index(
                self.deferred_draws_index,
                from.deferred_draws_index,
                to.deferred_draws_index,
            )?,
            dispatch_tree_index: rebase_index(
                self.dispatch_tree_index,
                from.dispatch_tree_index,
                to.dispatch_tree_index,
            )?,
            accessed_element_states_index: rebase_index(
                self.accessed_element_states_index,
                from.accessed_element_states_index,
                to.accessed_element_states_index,
            )?,
            line_layout_index: self
                .line_layout_index
                .rebased(&from.line_layout_index, &to.line_layout_index)?,
        })
    }

    /// Whether this index lies at or before `end` in every list it counts.
    pub(crate) fn not_after(&self, end: &Self) -> bool {
        self.hitboxes_index <= end.hitboxes_index
            && self.tooltips_index <= end.tooltips_index
            && self.deferred_draws_index <= end.deferred_draws_index
            && self.dispatch_tree_index <= end.dispatch_tree_index
            && self.accessed_element_states_index <= end.accessed_element_states_index
            && self.line_layout_index.not_after(&end.line_layout_index)
    }

    /// The element states used between two indices of one frame.
    pub(crate) fn accessed_element_states(range: &Range<Self>) -> Range<usize> {
        range.start.accessed_element_states_index..range.end.accessed_element_states_index
    }

    /// Argus patch: how many dispatch nodes the frame had at this index.
    pub(crate) fn dispatch_nodes(&self) -> usize {
        self.dispatch_tree_index
    }

    /// Argus patch: how many element states the frame had used at this index.
    pub(crate) fn element_states(&self) -> usize {
        self.accessed_element_states_index
    }

    /// Argus patch: this index without the element state used last. A
    /// cached view's range starts just after the use of its own state.
    pub(crate) fn before_last_state(&self) -> Self {
        let mut index = self.clone();
        index.accessed_element_states_index = index.accessed_element_states_index.saturating_sub(1);
        index
    }
}

impl PaintIndex {
    /// See [`PrepaintStateIndex::rebased`].
    pub(crate) fn rebased(&self, from: &Self, to: &Self) -> Option<Self> {
        Some(Self {
            scene_index: rebase_index(self.scene_index, from.scene_index, to.scene_index)?,
            #[cfg(any(test, feature = "test-support"))]
            debug_bounds_index: rebase_index(
                self.debug_bounds_index,
                from.debug_bounds_index,
                to.debug_bounds_index,
            )?,
            mouse_listeners_index: rebase_index(
                self.mouse_listeners_index,
                from.mouse_listeners_index,
                to.mouse_listeners_index,
            )?,
            input_handlers_index: rebase_index(
                self.input_handlers_index,
                from.input_handlers_index,
                to.input_handlers_index,
            )?,
            cursor_styles_index: rebase_index(
                self.cursor_styles_index,
                from.cursor_styles_index,
                to.cursor_styles_index,
            )?,
            window_control_hitboxes_index: rebase_index(
                self.window_control_hitboxes_index,
                from.window_control_hitboxes_index,
                to.window_control_hitboxes_index,
            )?,
            accessed_element_states_index: rebase_index(
                self.accessed_element_states_index,
                from.accessed_element_states_index,
                to.accessed_element_states_index,
            )?,
            tab_handle_index: rebase_index(
                self.tab_handle_index,
                from.tab_handle_index,
                to.tab_handle_index,
            )?,
            line_layout_index: self
                .line_layout_index
                .rebased(&from.line_layout_index, &to.line_layout_index)?,
        })
    }

    /// See [`PrepaintStateIndex::element_states`].
    pub(crate) fn element_states(&self) -> usize {
        self.accessed_element_states_index
    }

    /// See [`PrepaintStateIndex::before_last_state`].
    pub(crate) fn before_last_state(&self) -> Self {
        let mut index = self.clone();
        index.accessed_element_states_index = index.accessed_element_states_index.saturating_sub(1);
        index
    }

    /// See [`PrepaintStateIndex::not_after`].
    pub(crate) fn not_after(&self, end: &Self) -> bool {
        #[cfg(any(test, feature = "test-support"))]
        if self.debug_bounds_index > end.debug_bounds_index {
            return false;
        }
        self.scene_index <= end.scene_index
            && self.mouse_listeners_index <= end.mouse_listeners_index
            && self.input_handlers_index <= end.input_handlers_index
            && self.cursor_styles_index <= end.cursor_styles_index
            && self.window_control_hitboxes_index <= end.window_control_hitboxes_index
            && self.accessed_element_states_index <= end.accessed_element_states_index
            && self.tab_handle_index <= end.tab_handle_index
            && self.line_layout_index.not_after(&end.line_layout_index)
    }

    /// The element states used between two indices of one frame.
    pub(crate) fn accessed_element_states(range: &Range<Self>) -> Range<usize> {
        range.start.accessed_element_states_index..range.end.accessed_element_states_index
    }
}

impl Window {
    /// Calls `f` with the state of type `S` kept by each element that used
    /// one within `accessed`, a stretch of the element states the last frame
    /// used. A view replayed from the last frame moves what the cached views
    /// drawn inside it recorded there to where the replay put it.
    pub(crate) fn for_reused_element_states<S: 'static>(
        &mut self,
        accessed: Range<usize>,
        mut f: impl FnMut(&GlobalElementId, &mut S),
    ) {
        let state_type = TypeId::of::<S>();
        let Frame {
            accessed_element_states,
            element_states,
            ..
        } = &mut self.rendered_frame;
        let Some(keys) = accessed_element_states.get(accessed) else {
            return;
        };
        for key in keys {
            if key.1 != state_type {
                continue;
            }
            if let Some(state) = element_states
                .get_mut(key)
                .and_then(|state| state.inner.downcast_mut::<Option<S>>())
                .and_then(Option::as_mut)
            {
                f(&key.0, state);
            }
        }
    }

    /// Argus patch: whether the last frame used the state of type `S` of the
    /// element `global_id` just before `index` (a count of the element states
    /// it had used).
    pub(crate) fn rendered_state_used_before<S: 'static>(
        &self,
        index: usize,
        global_id: &GlobalElementId,
    ) -> bool {
        index
            .checked_sub(1)
            .and_then(|index| self.rendered_frame.accessed_element_states.get(index))
            .is_some_and(|(id, state_type)| id == global_id && *state_type == TypeId::of::<S>())
    }

    /// Argus patch, test support: how many element states the last frame used.
    #[cfg(any(test, feature = "test-support"))]
    pub fn used_element_states(&self) -> usize {
        self.rendered_frame.accessed_element_states.len()
    }

    /// Argus patch: the state of type `S` an element kept in the last frame.
    pub(crate) fn rendered_element_state<S: 'static>(
        &self,
        global_id: &GlobalElementId,
    ) -> Option<&S> {
        self.rendered_frame
            .element_states
            .get(&(global_id.clone(), TypeId::of::<S>()))
            .and_then(|state| state.inner.downcast_ref::<Option<S>>())
            .and_then(Option::as_ref)
    }

    /// Argus patch: [`Self::reuse_prepaint`] for one stretch of a view that
    /// replays around views drawn again in its place (`splice` in view.rs).
    /// The dispatch nodes it copies stay open for what is drawn next.
    pub(crate) fn reuse_prepaint_stretch(
        &mut self,
        range: Range<PrepaintStateIndex>,
        reuse: &mut StretchReuse,
    ) {
        // Argus patch: the replay adds no accessibility nodes (splices are
        // not used while assistive technology reads the window anyway).
        self.a11y.note_replay();
        self.next_frame.hitboxes.extend(
            self.rendered_frame.hitboxes[range.start.hitboxes_index..range.end.hitboxes_index]
                .iter()
                .cloned(),
        );
        self.next_frame.tooltip_requests.extend(
            self.rendered_frame.tooltip_requests
                [range.start.tooltips_index..range.end.tooltips_index]
                .iter_mut()
                .map(|request| request.take()),
        );
        self.next_frame.accessed_element_states.extend(
            self.rendered_frame.accessed_element_states[range.start.accessed_element_states_index
                ..range.end.accessed_element_states_index]
                .iter()
                .map(|(id, type_id)| (id.clone(), *type_id)),
        );
        self.text_system
            .reuse_layouts(range.start.line_layout_index..range.end.line_layout_index);

        if self.next_frame.dispatch_tree.reuse_stretch(
            reuse,
            range.start.dispatch_tree_index..range.end.dispatch_tree_index,
            &mut self.rendered_frame.dispatch_tree,
            self.focus,
        ) {
            self.next_frame.focus = self.focus;
        }

        let active = self.next_frame.dispatch_tree.active_node_id();
        self.next_frame.deferred_draws.extend(
            self.rendered_frame.deferred_draws
                [range.start.deferred_draws_index..range.end.deferred_draws_index]
                .iter()
                .map(|deferred_draw| DeferredDraw {
                    current_view: deferred_draw.current_view,
                    // `splice_fits` checked that it lies in a stretch.
                    parent_node: reuse
                        .refresh_node_id(deferred_draw.parent_node)
                        .or(active)
                        .expect("a deferred draw outside the replayed view"),
                    element_id_stack: deferred_draw.element_id_stack.clone(),
                    text_style_stack: deferred_draw.text_style_stack.clone(),
                    content_mask: deferred_draw.content_mask,
                    rem_size: deferred_draw.rem_size,
                    priority: deferred_draw.priority,
                    element: None,
                    absolute_offset: deferred_draw.absolute_offset,
                    prepaint_range: deferred_draw.prepaint_range.clone(),
                    paint_range: deferred_draw.paint_range.clone(),
                    accessed_entities: deferred_draw.accessed_entities.clone(),
                }),
        );
    }

    /// Argus patch: whether a view drawn in the last frame inside the
    /// prepaint range `outer` (of the view around it) can be drawn again in
    /// place while that view replays around it: `inner` is its range, it
    /// started inside the node drawn for it, and every deferred draw outside
    /// it belongs to a node outside it too.
    pub(crate) fn splice_fits(
        &self,
        view: EntityId,
        outer: &Range<PrepaintStateIndex>,
        inner: &Range<PrepaintStateIndex>,
    ) -> bool {
        let tree = &self.rendered_frame.dispatch_tree;
        let (start, end) = (
            inner.start.dispatch_tree_index,
            inner.end.dispatch_tree_index,
        );
        // The node drawn for the view itself comes just before its range.
        if start <= outer.start.dispatch_tree_index
            || tree.node_view(DispatchNodeId::from_index(start - 1)) != Some(view)
        {
            return false;
        }
        let draws = &self.rendered_frame.deferred_draws;
        let outside = (outer.start.deferred_draws_index..inner.start.deferred_draws_index)
            .chain(inner.end.deferred_draws_index..outer.end.deferred_draws_index);
        outside.into_iter().all(|index| {
            draws.get(index).is_some_and(|draw| {
                let node = draw.parent_node.index();
                (outer.start.dispatch_tree_index..outer.end.dispatch_tree_index).contains(&node)
                    && !(start..end).contains(&node)
            })
        })
    }

    /// Argus patch: the node of the last frame drawn for a view that is about
    /// to be drawn again in place (just before its range, see `splice_fits`):
    /// its copy, open after the stretch before, takes the new nodes.
    pub(crate) fn enter_spliced_view(
        &mut self,
        reuse: &mut StretchReuse,
        range: &Range<PrepaintStateIndex>,
    ) -> Option<DispatchNodeId> {
        let parent = DispatchNodeId::from_index(range.start.dispatch_tree_index.checked_sub(1)?);
        self.next_frame
            .dispatch_tree
            .reuse_enter(reuse, parent)
            .then(|| self.next_frame.dispatch_tree.active_node_id())
            .flatten()
    }

    /// Argus patch: closes the nodes the stretches of a replay left open.
    pub(crate) fn finish_prepaint_reuse(&mut self, reuse: &mut StretchReuse) {
        self.next_frame.dispatch_tree.finish_reuse(reuse);
    }

    /// Argus patch: makes `node` (of the frame being drawn) the active node
    /// for painting a view drawn again in place.
    pub(crate) fn activate_dispatch_node(&mut self, node: DispatchNodeId) {
        self.next_frame.dispatch_tree.set_active_node(node);
    }
}
