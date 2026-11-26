//! Argus patch: frame demand, redraws of a view that do not notify it
//! (animation frames, isolated frames, interactions), the number of the
//! draw in progress and frames drawn only to lay out.
use super::*;

impl WindowInvalidator {
    /// Marks `entity` for redrawing in the next frame because it animates.
    /// Unlike [`Self::invalidate_view`], observers aren't notified (its state
    /// didn't change), and cached views nested in it can be replayed.
    pub fn invalidate_view_for_animation(&self, entity: EntityId) {
        self.invalidate_view_for_frame(entity, false);
    }

    /// [`Self::invalidate_view_for_animation`] for an animation that changes
    /// nothing outside the cached view around `entity`: the cached views
    /// around that one replay around it (see `Window::request_isolated_animation_frame`).
    pub fn invalidate_view_for_isolated_animation(&self, entity: EntityId) {
        self.invalidate_view_for_frame(entity, true);
    }

    fn invalidate_view_for_frame(&self, entity: EntityId, isolated: bool) {
        let mut inner = self.inner.borrow_mut();
        inner.update_count += 1;
        if isolated {
            inner.isolated_views.insert(entity);
        } else {
            inner.animation_views.insert(entity);
        }
        if inner.draw_phase == DrawPhase::None {
            #[cfg(feature = "profiler")]
            let dirty_at = Self::record_frame_dirty(&mut inner);
            let became_dirty = !inner.dirty;
            inner.dirty = true;
            let waker = became_dirty.then(|| inner.platform_waker.clone()).flatten();
            #[cfg(feature = "profiler")]
            let window_id = inner.window_id;
            drop(inner);
            #[cfg(feature = "profiler")]
            if became_dirty {
                profiler::journal::record_frame_pending(window_id, dirty_at);
            }
            if let Some(waker) = waker {
                waker();
            }
        }
    }

    /// Whether a view notified during the draw in progress is `view` or lies
    /// inside it (in the last rendered frame).
    fn pending_within(&self, view: EntityId, tree: &DispatchTree) -> bool {
        let inner = self.inner.borrow();
        inner.dirty_views.iter().any(|pending| {
            *pending == view
                || tree
                    .view_path_reversed(*pending)
                    .any(|ancestor| ancestor == view)
        })
    }

    pub fn take_animation_views(&self) -> FxHashSet<EntityId> {
        mem::take(&mut self.inner.borrow_mut().animation_views)
    }

    pub fn take_isolated_views(&self) -> FxHashSet<EntityId> {
        mem::take(&mut self.inner.borrow_mut().isolated_views)
    }
}

impl Window {
    /// Argus patch: whether the window still has something to draw or
    /// present: a dirty view, a next-frame callback (an animation's next
    /// step), or a drawn frame not yet presented. Timers that will change the
    /// window later are not counted.
    pub fn has_frame_demand(&self) -> bool {
        self.invalidator.is_dirty()
            || !self.next_frame_callbacks.borrow().is_empty()
            || self.needs_present.get()
    }

    /// Argus patch: [`Self::request_animation_frame`] for an animation that
    /// changes nothing outside the nearest cached view around this view (the
    /// view itself if it is cached), such as a glide of a list it scrolls.
    /// Cached views further out replay their last frame around that one,
    /// which is drawn again in its place: their own trees are not built,
    /// laid out or painted again. What they paint from the animated state
    /// (a scrollbar of the list drawn outside it, say) stays as it was, so
    /// it must ask for its own frames (`ScrollHandle::gliding`).
    ///
    /// A cached view around it renders as usual when anything else asks it
    /// to, when it was not drawn in the last frame, or while assistive
    /// technology reads the window.
    pub fn request_isolated_animation_frame(&self) {
        let entity = self.current_view();
        self.on_next_frame(move |window, _| {
            window
                .invalidator
                .invalidate_view_for_isolated_animation(entity)
        });
    }

    /// Redraw a view without notifying observers or invalidating its cached
    /// children. The caller must separately notify any child whose inputs changed.
    /// Use outside drawing when only uncached content owned by this view changed.
    pub fn request_view_redraw(&self, entity: EntityId) {
        self.invalidator.invalidate_view_for_animation(entity);
    }

    /// Argus patch: [`Self::request_view_redraw`] for a redraw that changes
    /// nothing outside the nearest cached view around `entity`, as an
    /// isolated animation frame does ([`Self::request_isolated_animation_frame`]).
    pub fn request_isolated_view_redraw(&self, entity: EntityId) {
        self.invalidator
            .invalidate_view_for_isolated_animation(entity);
    }

    /// Draws `view` again because an element it painted changed how it looks
    /// or what it listens for, without the view's state changing: a hover, a
    /// press, a tooltip coming or going. Its observers aren't notified and the
    /// cached views inside it replay (their keys hold what they inherit from
    /// it), where [`Self::refresh`] would render every view of the window.
    ///
    /// During a draw (a tooltip found gone while the window prepaints it) the
    /// view draws again in the next frame. `refresh` does nothing then, which
    /// left the tooltip's request in the view's prepaint until something else
    /// drew the view: every frame that replayed the view laid the tooltip out
    /// again to find it gone, and showed it at once if its element was waiting
    /// to show it anew. Views now replay for as long as nothing changes them.
    pub(crate) fn redraw_view_for_interaction(&self, view: EntityId) {
        if self.invalidator.not_drawing() {
            self.invalidator.invalidate_view_for_animation(view);
        } else {
            self.on_next_frame(move |window, _| {
                window.invalidator.invalidate_view_for_animation(view)
            });
        }
    }

    /// [`Self::redraw_view_for_interaction`] for an element that may have
    /// been painted outside any view, where the window is refreshed.
    pub(crate) fn redraw_for_interaction(&mut self, view: Option<EntityId>) {
        match view {
            Some(view) => self.redraw_view_for_interaction(view),
            None => self.refresh(),
        }
    }

    /// Whether a cached view must render again although its own cache key
    /// matches: it, or a view inside it, was notified during this draw.
    pub(crate) fn view_notified_during_draw(&self, view: EntityId) -> bool {
        self.invalidator
            .pending_within(view, &self.rendered_frame.dispatch_tree)
    }

    /// The number of the draw in progress; a cached view replays its last
    /// ranges only if it was drawn in the one before.
    pub(crate) fn paint_generation(&self) -> u64 {
        self.paint_generation
    }

    /// Keep the last presented scene while this prepaint measures content for
    /// a scheduled layout pass. The next draw can present normally.
    pub fn defer_presentation_until_layout(&mut self) {
        self.invalidator.debug_assert_prepaint();
        self.next_frame.layout_only = true;
    }

    /// Whether this prepaint is measuring a frame that will not be presented.
    pub fn is_layout_only(&self) -> bool {
        self.invalidator.debug_assert_prepaint();
        self.next_frame.layout_only
    }
}
