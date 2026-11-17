use crate::text_style_stack::TextStyleStack;
use crate::{
    AnyElement, AnyEntity, AnyWeakEntity, App, AvailableSpace, Bounds, ContentMask, Context,
    Element, ElementId, Entity, EntityId, GlobalElementId, InspectorElementId, IntoElement,
    LayoutId, PaintIndex, Pixels, PrepaintStateIndex, Render, RenderOnce, Size, Style,
    StyleRefinement, TextStyle, WeakEntity,
};
use crate::{DispatchNodeId, ElementIdStack, GroupHitboxes, HitboxId, SharedString, StretchReuse};
use crate::{Empty, Window};
use anyhow::Result;
use collections::FxHashSet;
use refineable::Refineable;
use smallvec::SmallVec;
use std::mem;
use std::{
    any::{TypeId, type_name},
    fmt,
    ops::Range,
};

/// A dynamically-typed view handle that can be downcast to a specific `Entity<V>`.
///
/// This is the type-erased counterpart to [`ViewElement`]: it holds an entity plus
/// a function pointer to its render, and is itself a [`View`], so embedding it as an
/// element goes through the same [`ViewElement`] machinery as any other view.
#[derive(Clone, Debug)]
pub struct AnyView {
    entity: AnyEntity,
    render: fn(&AnyView, &mut Window, &mut App) -> AnyElement,
    view_type_name: &'static str,
}

impl<V: Render> From<Entity<V>> for AnyView {
    fn from(value: Entity<V>) -> Self {
        AnyView {
            entity: value.into_any(),
            render: any_view::render::<V>,
            view_type_name: type_name::<V>(),
        }
    }
}

impl AnyView {
    /// Embed this view as a cached [`ViewElement`] laid out at `style`.
    ///
    /// The rendered subtree is recycled from the previous frame unless
    /// [Context::notify] was called on the backing entity since it was rendered
    /// (or [Window::refresh] is called, which ignores caching).
    pub fn cached(self, style: StyleRefinement) -> ViewElement<AnyView> {
        let weak = self.downgrade();
        self.into_element().cached(style).splicing(weak)
    }

    /// Convert this to a weak handle.
    pub fn downgrade(&self) -> AnyWeakView {
        AnyWeakView {
            entity: self.entity.downgrade(),
            render: self.render,
            view_type_name: self.view_type_name,
        }
    }

    /// Convert this to a [Entity] of a specific type.
    /// If this handle does not contain a view of the specified type, returns itself in an `Err` variant.
    pub fn downcast<T: 'static>(self) -> Result<Entity<T>, Self> {
        match self.entity.downcast() {
            Ok(entity) => Ok(entity),
            Err(entity) => Err(Self {
                entity,
                render: self.render,
                view_type_name: self.view_type_name,
            }),
        }
    }

    /// Gets the [TypeId] of the underlying view.
    pub fn entity_type(&self) -> TypeId {
        self.entity.entity_type
    }

    /// The [`EntityId`] of this view.
    pub fn entity_id(&self) -> EntityId {
        self.entity.entity_id()
    }
}

impl PartialEq for AnyView {
    fn eq(&self, other: &Self) -> bool {
        self.entity == other.entity
    }
}

impl Eq for AnyView {}

/// `AnyView` is the type-erased [`View`]: its `render` is a function pointer rather
/// than a concrete type, but it participates in the reactive graph exactly like any
/// other view via [`ViewElement`].
impl View for AnyView {
    fn entity_id(&self) -> Option<EntityId> {
        Some(self.entity.entity_id())
    }

    #[cfg(not(gpui_hot_reload))]
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        (self.render)(&self, window, cx)
    }

    // The stored function pointer was captured from the original binary, so it must be looked up
    // in the jump table to reach the patched render.
    #[cfg(gpui_hot_reload)]
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        subsecond::HotFn::current(self.render).call((&self, window, cx))
    }
}

impl<V: 'static + Render> IntoElement for Entity<V> {
    type Element = ViewElement<Entity<V>>;

    fn into_element(self) -> Self::Element {
        ViewElement::with_type_name(self, type_name::<V>())
    }

    #[inline(never)]
    fn into_any_element(self) -> AnyElement {
        self.into_element().into_any()
    }
}

impl IntoElement for AnyView {
    type Element = ViewElement<AnyView>;

    fn into_element(self) -> Self::Element {
        let view_type_name = self.view_type_name;
        ViewElement::with_type_name(self, view_type_name)
    }
}

/// A weak, dynamically-typed view handle.
#[derive(Clone)]
pub struct AnyWeakView {
    entity: AnyWeakEntity,
    render: fn(&AnyView, &mut Window, &mut App) -> AnyElement,
    view_type_name: &'static str,
}

impl AnyWeakView {
    /// Upgrade to a strong `AnyView` handle, if the view is still alive.
    pub fn upgrade(&self) -> Option<AnyView> {
        let entity = self.entity.upgrade()?;
        Some(AnyView {
            entity,
            render: self.render,
            view_type_name: self.view_type_name,
        })
    }
}

impl<V: 'static + Render> From<WeakEntity<V>> for AnyWeakView {
    fn from(view: WeakEntity<V>) -> Self {
        AnyWeakView {
            entity: view.into(),
            render: any_view::render::<V>,
            view_type_name: type_name::<V>(),
        }
    }
}

impl PartialEq for AnyWeakView {
    fn eq(&self, other: &Self) -> bool {
        self.entity == other.entity
    }
}

impl std::fmt::Debug for AnyWeakView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AnyWeakView")
            .field("entity_id", &self.entity.entity_id)
            .finish_non_exhaustive()
    }
}

mod any_view {
    use crate::{AnyElement, AnyView, App, IntoElement, Render, Window};

    pub(crate) fn render<V: 'static + Render>(
        view: &AnyView,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let view = view.clone().downcast::<V>().unwrap();
        // Record the view's Render type name so the accessibility debug dump can
        // attribute nodes to the view that produced them.
        #[cfg(debug_assertions)]
        window
            .a11y
            .view_type_names
            .insert(view.entity_id(), std::any::type_name::<V>());
        view.update(cx, |view, cx| view.render(window, cx).into_any_element())
    }
}

/// A renderable that participates in GPUI's reactive graph — the unifying model
/// behind [`Render`] and [`RenderOnce`].
///
/// When `entity_id()` returns `Some`, that id becomes the view's identity: it gets
/// a unique element-id space (so internal `use_state` / `.id(..)` never collide
/// across siblings) and `cx.notify()` on that entity re-renders only this view's
/// subtree. `None` behaves like a stateless component.
///
/// You rarely implement `View` directly. `Entity<T: Render>` and any `T: RenderOnce`
/// get a blanket impl below; implement it by hand only when a component needs both
/// parent-supplied props *and* a backing entity for identity.
pub trait View: 'static + Sized {
    /// This view's identity, if it has one. A view typically holds the backing
    /// entity as a field and returns its [`EntityId`] here.
    ///
    /// The id becomes this view's [`ElementId`], so two views keyed on the same
    /// entity must not be rendered at the same position in the element tree
    /// (e.g. as siblings under the same parent): their internal element state
    /// (`use_state`, scroll offsets, etc.) would silently collide. Nesting is
    /// fine — the id is scoped by the parent path.
    fn entity_id(&self) -> Option<EntityId>;

    /// Render this view into an element tree, consuming `self`.
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement;
}

/// A stateless component (`RenderOnce`) is a `View` with no identity.
impl<T: RenderOnce> View for T {
    fn entity_id(&self) -> Option<EntityId> {
        None
    }

    #[inline]
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        RenderOnce::render(self, window, cx)
    }
}

/// An entity that renders itself (`Render`) is a `View` keyed on its own id.
impl<T: Render> View for Entity<T> {
    fn entity_id(&self) -> Option<EntityId> {
        Some(Entity::entity_id(self))
    }

    #[inline]
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        self.update(cx, |this, cx| {
            Render::render(this, window, cx).into_any_element()
        })
    }
}

impl<T: Render> Entity<T> {
    /// Embed this entity as a cached [`ViewElement`] laid out at `style`.
    ///
    /// The rendered subtree is reused until the entity is notified (or the
    /// cached bounds / text style change). Caching requires a definite size:
    /// a cached view is laid out from `style` and is *not* measured from its
    /// contents. Use [`ViewElement::new`] (or `.child(entity)`) for the
    /// uncached case.
    #[track_caller]
    pub fn cached(self, style: StyleRefinement) -> ViewElement<Entity<T>> {
        let weak = AnyWeakView::from(self.downgrade());
        ViewElement::with_type_name(self, type_name::<T>())
            .cached(style)
            .splicing(weak)
    }
}

/// The element type for [`View`] implementations. Wraps a `View` and hooks it
/// into layout, prepaint, and paint. Constructed via [`ViewElement::new`].
#[doc(hidden)]
pub struct ViewElement<V: View> {
    view: Option<V>,
    entity_id: Option<EntityId>,
    cached_style: Option<StyleRefinement>,
    /// Argus patch: a cached view, to draw it again in place while the views
    /// around it replay (`splice_holes`).
    weak_view: Option<AnyWeakView>,
    #[cfg(feature = "profiler")]
    view_type_name: &'static str,
    #[cfg(debug_assertions)]
    source: &'static core::panic::Location<'static>,
}

impl<V: View> ViewElement<V> {
    /// Wrap a [`View`] as an element.
    #[track_caller]
    pub fn new(view: V) -> Self {
        Self::with_type_name(view, type_name::<V>())
    }

    /// Wraps a view whose draw time profiling attributes to `view_type_name`,
    /// such as an entity's type rather than [`Entity`]'s.
    #[track_caller]
    #[cfg_attr(not(feature = "profiler"), expect(unused_variables))]
    pub(crate) fn with_type_name(view: V, view_type_name: &'static str) -> Self {
        let entity_id = view.entity_id();
        ViewElement {
            entity_id,
            cached_style: None,
            weak_view: None,
            #[cfg(feature = "profiler")]
            view_type_name,
            view: Some(view),
            #[cfg(debug_assertions)]
            source: core::panic::Location::caller(),
        }
    }

    /// Enable caching of this view's rendered subtree, laid out at `style`.
    /// The composer supplies the layout style because caching skips rendering
    /// the contents to measure them.
    ///
    /// Crate-private on purpose: caching is only sound for entity-backed views,
    /// where [`Context::notify`] is the contract that busts the cache. A stateless
    /// view has no such contract, so a frozen subtree could never be invalidated.
    /// Reach this through [`Entity::cached`] or [`AnyView::cached`], which are
    /// entity-backed by construction.
    pub(crate) fn cached(mut self, style: StyleRefinement) -> Self {
        self.cached_style = Some(style);
        self
    }

    /// Argus patch: how to render this cached view without the view around
    /// it, so that it can be drawn again in place while that one replays.
    fn splicing(mut self, view: AnyWeakView) -> Self {
        self.weak_view = Some(view);
        self
    }
}

impl<V: View> IntoElement for ViewElement<V> {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

struct ViewElementState {
    prepaint_range: Range<PrepaintStateIndex>,
    paint_range: Range<PaintIndex>,
    /// The draws whose frame each range indexes (see
    /// `Window::paint_generation`): the one that last prepainted the view
    /// and the one that last painted it, by rendering it, by replaying it, or
    /// by moving its range along with a replayed view around it
    /// (`rebase_nested_prepaint`). They differ when a prepaint was not
    /// followed by a paint (under an element that is hidden, say): the paint
    /// range then indexes an older frame and must not be replayed.
    prepainted: u64,
    painted: u64,
    cache_key: ViewElementCacheKey,
    /// The entities the view accessed while it was last drawn, in any phase,
    /// restored when its prepaint is reused so the window stays subscribed.
    accessed_entities: FxHashSet<EntityId>,
    /// Argus patch: the view.
    view: EntityId,
    /// Argus patch: how to draw the view again in place while the views
    /// around it replay (`splice_holes`), from when it last rendered.
    splice: Option<SpliceContext>,
}

/// Argus patch: what a cached view was rendered and painted inside, for
/// drawing it again without the views around it, which replay their last
/// frame around it (`splice_holes`). The cache key holds the rest: bounds,
/// content mask, text style and opacity at prepaint.
struct SpliceContext {
    view: AnyWeakView,
    /// The element ids down to the view's own.
    element_id_stack: ElementIdStack,
    text_style_stack: TextStyleStack,
    /// The views rendering around it, innermost last.
    rendered_entity_stack: Vec<EntityId>,
    rem_size: Pixels,
    /// Set when the view is painted after it rendered.
    paint: Option<SplicePaint>,
}

#[derive(Clone)]
struct SplicePaint {
    content_mask: ContentMask<Pixels>,
    opacity: f32,
    hover_fills_in_place: usize,
    /// The groups painted around it (`group_hover` and the like).
    groups: SmallVec<[(SharedString, HitboxId); 2]>,
}

/// Argus patch: what a cached view prepainted for its paint.
#[doc(hidden)]
pub struct ViewPrepaint(Prepainted);

enum Prepainted {
    /// Rendered: the element to paint.
    Element(AnyElement),
    /// Replayed around the cached views inside it that were drawn again in
    /// their places, in order.
    Spliced(Vec<SplicedView>),
}

/// Argus patch: a cached view drawn again in place in a view that replays
/// around it, to paint where its last paint lay in the replayed view's.
struct SplicedView {
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
struct Hole {
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

struct ViewElementCacheKey {
    bounds: Bounds<Pixels>,
    content_mask: ContentMask<Pixels>,
    text_style: TextStyle,
    opacity: f32,
}

#[cfg(not(gpui_hot_reload))]
#[inline]
fn render_view<V: View>(view: V, window: &mut Window, cx: &mut App) -> AnyElement {
    view.render(window, cx).into_any_element()
}

/// Every view and component render passes through this hot-patchable boundary, so the
/// patched code for that render and everything it calls runs on the next frame.
#[cfg(gpui_hot_reload)]
fn render_view<V: View>(view: V, window: &mut Window, cx: &mut App) -> AnyElement {
    #[inline(never)]
    fn render_view_impl<V: View>(view: V, window: &mut Window, cx: &mut App) -> AnyElement {
        view.render(window, cx).into_any_element()
    }

    subsecond::HotFn::current(render_view_impl::<V>).call((view, window, cx))
}

impl<V: View> Element for ViewElement<V> {
    type RequestLayoutState = Option<AnyElement>;
    type PrepaintState = Option<ViewPrepaint>;

    fn id(&self) -> Option<ElementId> {
        self.entity_id.map(ElementId::View)
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        #[cfg(debug_assertions)]
        return Some(self.source);

        #[cfg(not(debug_assertions))]
        return None;
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        if let Some(entity_id) = self.entity_id {
            // Stateful path: create a reactive boundary.
            #[cfg(feature = "profiler")]
            let start = window.window_profiler.begin_view();
            let view = &mut self.view;
            let layout = request_layout_view(
                entity_id,
                self.view_type_name,
                self.cached_style.as_ref(),
                window,
                cx,
                &mut |window, cx| render_view(view.take().unwrap(), window, cx),
            );
            #[cfg(feature = "profiler")]
            window.window_profiler.end_view(
                start,
                self.view_type_name,
                crate::profiler::ViewPhase::RequestLayout,
            );
            layout
        } else {
            // Stateless path: isolate subtree via type name (no entity identity).
            request_layout_component(type_name::<V>(), window, cx, &mut |window, cx| {
                render_view(self.view.take().unwrap(), window, cx)
            })
        }
    }

    fn prepaint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        element: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<ViewPrepaint> {
        if let Some(entity_id) = self.entity_id {
            // Stateful path.
            #[cfg(feature = "profiler")]
            let start = window.window_profiler.begin_view();
            let prepainted = prepaint_view(
                entity_id,
                self.view_type_name,
                self.weak_view.as_ref(),
                global_id,
                bounds,
                element,
                window,
                cx,
                &mut |window, cx| render_view(self.view.take().unwrap(), window, cx),
            );
            #[cfg(feature = "profiler")]
            window.window_profiler.end_view(
                start,
                self.view_type_name,
                crate::profiler::ViewPhase::Prepaint,
            );
            prepainted
        } else {
            // Stateless path: just prepaint the element.
            prepaint_component(type_name::<V>(), element, window, cx)
        }
    }

    fn paint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        element: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(entity_id) = self.entity_id {
            // Stateful path.
            #[cfg(feature = "profiler")]
            let start = window.window_profiler.begin_view();
            paint_view(
                entity_id,
                self.cached_style.is_some(),
                global_id,
                element,
                window,
                cx,
            );
            #[cfg(feature = "profiler")]
            window.window_profiler.end_view(
                start,
                self.view_type_name,
                crate::profiler::ViewPhase::Paint,
            );
        } else {
            // Stateless path: just paint the element.
            paint_component(std::any::type_name::<V>(), element, window, cx);
        }
    }
}

/// A view that renders nothing
pub struct EmptyView;

impl Render for EmptyView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

#[inline(never)]
fn request_layout_view(
    entity_id: EntityId,
    _name: &'static str,
    cached_style: Option<&StyleRefinement>,
    window: &mut Window,
    cx: &mut App,
    render: &mut dyn FnMut(&mut Window, &mut App) -> AnyElement,
) -> (LayoutId, Option<AnyElement>) {
    window.with_rendered_view(entity_id, |window| {
        let caching_disabled = window.is_inspector_picking(cx);
        match cached_style {
            Some(style) if !caching_disabled => {
                let mut root_style = Style::default();
                root_style.refine(style);
                let layout_id = window.request_layout(root_style, None, cx);
                (layout_id, None)
            }
            _ => {
                #[cfg(feature = "profiler")]
                let started = std::time::Instant::now();
                let mut element = render(window, cx);
                #[cfg(feature = "profiler")]
                window
                    .window_profiler
                    .record_view_render(_name, started.elapsed());
                let layout_id = element.request_layout(window, cx);
                (layout_id, Some(element))
            }
        }
    })
}

#[inline(never)]
fn request_layout_component(
    name: &'static str,
    window: &mut Window,
    cx: &mut App,
    render: &mut dyn FnMut(&mut Window, &mut App) -> AnyElement,
) -> (LayoutId, Option<AnyElement>) {
    window.with_id(ElementId::from(name), |window| {
        let mut element = render(window, cx);
        let layout_id = element.request_layout(window, cx);
        (layout_id, Some(element))
    })
}

#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn prepaint_view(
    entity_id: EntityId,
    _name: &'static str,
    weak_view: Option<&AnyWeakView>,
    global_id: Option<&GlobalElementId>,
    bounds: Bounds<Pixels>,
    element: &mut Option<AnyElement>,
    window: &mut Window,
    cx: &mut App,
    render: &mut dyn FnMut(&mut Window, &mut App) -> AnyElement,
) -> Option<ViewPrepaint> {
    window.set_view_id(entity_id);
    window.with_rendered_view(entity_id, |window| {
        if let Some(mut element) = element.take() {
            element.prepaint(window, cx);
            return Some(ViewPrepaint(Prepainted::Element(element)));
        }

        let global_id = global_id.unwrap();
        let prepainted =
            window.with_element_state::<ViewElementState, _>(global_id, |element_state, window| {
                let content_mask = window.content_mask();
                let text_style = window.text_style();
                let opacity = window.element_opacity();

                // The ranges index the last frame: only a view prepainted and
                // painted in it can replay them.
                let generation = window.paint_generation();
                // Opacity is compared by bits so that a NaN opacity still matches itself
                // and the view keeps using its cache.
                let current = element_state.filter(|element_state| {
                    element_state.cache_key.bounds == bounds
                        && element_state.cache_key.content_mask == content_mask
                        && element_state.cache_key.text_style == text_style
                        && element_state.cache_key.opacity.to_bits() == opacity.to_bits()
                        && !window.refreshing
                        && element_state.prepainted == generation.wrapping_sub(1)
                        && element_state.painted == generation.wrapping_sub(1)
                        && !window.view_notified_during_draw(entity_id)
                });
                if let Some(mut element_state) = current {
                    if !window.dirty_views.contains(&entity_id) {
                        element_state.prepainted = generation;
                        #[cfg(feature = "profiler")]
                        window.window_profiler.record_view_reuse();
                        let previous = element_state.prepaint_range.clone();
                        let prepaint_start = window.prepaint_index();
                        window.reuse_prepaint(previous.clone());
                        cx.entities
                            .extend_accessed(element_state.accessed_entities.iter().copied());
                        let prepaint_end = window.prepaint_index();
                        rebase_nested_prepaint(window, &previous, &prepaint_start, generation);
                        element_state.prepaint_range = prepaint_start..prepaint_end;

                        return (None, element_state);
                    }
                    // Argus patch: drawn again only for isolated animations
                    // inside it, it replays around the views that hold them.
                    if let Some(holes) = splice_holes(window, entity_id, &element_state, generation)
                    {
                        #[cfg(feature = "profiler")]
                        window.window_profiler.record_view_reuse();
                        let spliced =
                            splice_prepaint(window, cx, &mut element_state, holes, generation);
                        return (
                            Some(ViewPrepaint(Prepainted::Spliced(spliced))),
                            element_state,
                        );
                    }
                }

                // A notified view renders the cached views inside it too: they
                // may show its state. Rendering again only to animate, or because
                // a view inside it changed, replays those still current.
                let refresh = window.refreshing || window.notified_views.contains(&entity_id);
                let refreshing = mem::replace(&mut window.refreshing, refresh);
                // Argus patch: what it renders inside, to draw it again in
                // place later (`splice_holes`). Not under an image cache,
                // which this does not keep.
                let splice = weak_view
                    .filter(|_| window.image_cache_stack.is_empty())
                    .map(|view| SpliceContext {
                        view: view.clone(),
                        element_id_stack: window.element_id_stack.clone(),
                        text_style_stack: window.text_style_stack.clone(),
                        rendered_entity_stack: window
                            .rendered_entity_stack
                            .split_last()
                            .map_or_else(Vec::new, |(_, around)| around.to_vec()),
                        rem_size: window.rem_size(),
                        paint: None,
                    });
                let prepaint_start = window.prepaint_index();
                let (element, accessed_entities) = cx.detect_accessed_entities(|cx| {
                    #[cfg(feature = "profiler")]
                    let started = std::time::Instant::now();
                    let mut element = render(window, cx);
                    #[cfg(feature = "profiler")]
                    window
                        .window_profiler
                        .record_view_render(_name, started.elapsed());
                    element.layout_as_root(Size::<AvailableSpace>::from(bounds.size), window, cx);
                    element.prepaint_at(bounds.origin, window, cx);
                    element
                });

                let prepaint_end = window.prepaint_index();
                window.refreshing = refreshing;

                (
                    Some(ViewPrepaint(Prepainted::Element(element))),
                    ViewElementState {
                        accessed_entities,
                        prepaint_range: prepaint_start..prepaint_end,
                        paint_range: PaintIndex::default()..PaintIndex::default(),
                        prepainted: generation,
                        painted: 0,
                        cache_key: ViewElementCacheKey {
                            bounds,
                            content_mask,
                            text_style,
                            opacity,
                        },
                        view: entity_id,
                        splice,
                    },
                )
            });
        window
            .next_frame
            .cached_views
            .insert(entity_id, global_id.clone());
        prepainted
    })
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
fn splice_holes(
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
fn splice_prepaint(
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
fn splice_paint(
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
fn rebase_nested_prepaint(
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
fn rebase_nested_paint(
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

#[inline(never)]
fn prepaint_component(
    name: &'static str,
    element: &mut Option<AnyElement>,
    window: &mut Window,
    cx: &mut App,
) -> Option<ViewPrepaint> {
    window.with_id(ElementId::from(name), |window| {
        element.as_mut().unwrap().prepaint(window, cx);
    });
    Some(ViewPrepaint(Prepainted::Element(element.take().unwrap())))
}

#[inline(never)]
fn paint_view(
    entity_id: EntityId,
    cached: bool,
    global_id: Option<&GlobalElementId>,
    prepaint: &mut Option<ViewPrepaint>,
    window: &mut Window,
    cx: &mut App,
) {
    window.with_rendered_view(entity_id, |window| {
        let caching_disabled = window.is_inspector_picking(cx);
        if cached && !caching_disabled {
            window.with_element_state::<ViewElementState, _>(
                global_id.unwrap(),
                |element_state, window| {
                    let mut element_state = element_state.unwrap();

                    let paint_start = window.paint_index();
                    let opacity = window.element_opacity();
                    let generation = window.paint_generation();

                    // Prepaint can only predict the opacity paint applies: an ancestor's
                    // hover or drag-over style depends on this frame's hit test, which
                    // isn't known until prepaint finishes. The key records what was
                    // actually painted, and output replayed under a different opacity is
                    // redrawn on the next frame.
                    match prepaint.as_mut().map(|prepaint| &mut prepaint.0) {
                        Some(Prepainted::Element(element)) => {
                            let refreshing = mem::replace(&mut window.refreshing, true);
                            let ((), accessed_entities) =
                                cx.detect_accessed_entities(|cx| element.paint(window, cx));
                            element_state.accessed_entities.extend(accessed_entities);
                            window.refreshing = refreshing;
                            element_state.cache_key.opacity = opacity;
                            // Argus patch: what it was painted inside, to draw
                            // it again in place (`splice_holes`).
                            if let Some(splice) = element_state.splice.as_mut() {
                                splice.paint = Some(SplicePaint {
                                    content_mask: window.content_mask(),
                                    opacity,
                                    hover_fills_in_place: window.hover_fills_in_place,
                                    groups: GroupHitboxes::innermost(cx),
                                });
                            }
                        }
                        Some(Prepainted::Spliced(spliced)) => {
                            splice_paint(window, cx, &mut element_state, spliced, generation);
                            if element_state.cache_key.opacity.to_bits() != opacity.to_bits() {
                                window.request_animation_frame();
                            }
                        }
                        None => {
                            let previous = element_state.paint_range.clone();
                            window.reuse_paint(previous.clone());
                            rebase_nested_paint(window, &previous, &paint_start, generation);
                            if element_state.cache_key.opacity.to_bits() != opacity.to_bits() {
                                window.request_animation_frame();
                            }
                        }
                    }

                    let paint_end = window.paint_index();
                    element_state.paint_range = paint_start..paint_end;
                    element_state.painted = generation;

                    ((), element_state)
                },
            )
        } else {
            match prepaint.as_mut().map(|prepaint| &mut prepaint.0) {
                Some(Prepainted::Element(element)) => element.paint(window, cx),
                _ => panic!("an uncached view is painted from its prepainted element"),
            }
        }
    });
}

#[inline(never)]
fn paint_component(
    name: &'static str,
    prepaint: &mut Option<ViewPrepaint>,
    window: &mut Window,
    cx: &mut App,
) {
    window.with_id(ElementId::Name(name.into()), |window| {
        match prepaint.as_mut().map(|prepaint| &mut prepaint.0) {
            Some(Prepainted::Element(element)) => element.paint(window, cx),
            _ => panic!("a component is painted from its prepainted element"),
        }
    });
}
