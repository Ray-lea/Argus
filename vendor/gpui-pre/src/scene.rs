// todo("windows"): remove
#![cfg_attr(windows, allow(dead_code))]

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    AtlasTextureId, AtlasTile, Background, Bounds, ContentMask, Corners, Edges, Hsla, Pixels,
    Point, Radians, ScaledPixels, Size, bounds_tree::BoundsTree, point, size,
};
use std::{
    fmt::Debug,
    iter::Peekable,
    ops::{Add, Range, Sub},
    slice,
};

#[allow(non_camel_case_types, unused)]
#[expect(missing_docs)]
pub type PathVertex_ScaledPixels = PathVertex<ScaledPixels>;

#[expect(missing_docs)]
pub type DrawOrder = u32;

/// A boolean stored as a `u32` so that GPU-facing structs contain no
/// compiler-inserted padding bytes, which would be undefined behavior to
/// reinterpret as `&[u8]` when writing instance buffers. Guaranteed to be
/// `0` or `1` by construction; shaders read it as a `u32`/`uint`.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
#[repr(transparent)]
pub struct PaddedBool32(u32);

impl From<bool> for PaddedBool32 {
    fn from(value: bool) -> Self {
        PaddedBool32(value as u32)
    }
}

#[derive(Default)]
#[expect(missing_docs)]
pub struct Scene {
    /// Unique identity for a complete paint, preserved by bounds-only animation.
    pub paint_revision: u64,
    /// Earliest draw order that can change without a complete paint.
    pub animated_order: Option<DrawOrder>,
    pub(crate) paint_operations: Vec<PaintOperation>,
    primitive_bounds: BoundsTree<ScaledPixels>,
    layer_stack: Vec<DrawOrder>,
    /// The visual transforms in effect, each composed with those outside it.
    transform_stack: Vec<VisualTransform>,
    pub shadows: Vec<Shadow>,
    pub quads: Vec<Quad>,
    pub paths: Vec<Path<ScaledPixels>>,
    pub underlines: Vec<Underline>,
    pub monochrome_sprites: Vec<MonochromeSprite>,
    pub subpixel_sprites: Vec<SubpixelSprite>,
    pub polychrome_sprites: Vec<PolychromeSprite>,
    pub surfaces: Vec<PaintSurface>,
}

#[expect(missing_docs)]
impl Scene {
    pub fn clear(&mut self) {
        static REVISION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        self.paint_revision = REVISION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.animated_order = None;
        self.paint_operations.clear();
        self.primitive_bounds.clear();
        self.layer_stack.clear();
        self.transform_stack.clear();
        self.paths.clear();
        self.shadows.clear();
        self.quads.clear();
        self.underlines.clear();
        self.monochrome_sprites.clear();
        self.subpixel_sprites.clear();
        self.polychrome_sprites.clear();
        self.surfaces.clear();
    }

    pub fn len(&self) -> usize {
        self.paint_operations.len()
    }

    pub fn push_layer(&mut self, bounds: Bounds<ScaledPixels>) {
        let placed = match self.transform_stack.last() {
            Some(transform) => transform.bounds(bounds),
            None => bounds,
        };
        let order = self.primitive_bounds.insert(placed);
        self.layer_stack.push(order);
        self.paint_operations
            .push(PaintOperation::StartLayer(bounds));
    }

    /// Moves and fades everything inserted until the matching
    /// [`Self::pop_transform`]. The operations keep the untransformed
    /// primitives, so replaying them follows the transforms then in effect.
    pub fn push_transform(&mut self, transform: VisualTransform) {
        let composed = match self.transform_stack.last() {
            Some(outer) => outer.then(transform),
            None => transform,
        };
        self.transform_stack.push(composed);
        self.paint_operations
            .push(PaintOperation::PushTransform(transform));
    }

    /// Like [`Self::push_transform`] with the current value of `transform`
    /// about `anchor`; a replay of the operations reads its value again.
    pub fn push_live_transform(
        &mut self,
        transform: LiveTransform,
        anchor: Point<ScaledPixels>,
        scale_factor: f32,
    ) {
        let visual = transform.get().visual(anchor, scale_factor);
        let composed = match self.transform_stack.last() {
            Some(outer) => outer.then(visual),
            None => visual,
        };
        self.transform_stack.push(composed);
        self.paint_operations.push(PaintOperation::PushLiveTransform {
            transform,
            anchor,
            scale_factor,
        });
    }

    pub fn pop_transform(&mut self) {
        self.transform_stack.pop();
        self.paint_operations.push(PaintOperation::PopTransform);
    }

    pub fn pop_layer(&mut self) {
        self.layer_stack.pop();
        self.paint_operations.push(PaintOperation::EndLayer);
    }

    pub fn insert_primitive(&mut self, primitive: impl Into<Primitive>) {
        let primitive = primitive.into();
        let Some(transform) = self.transform_stack.last().copied() else {
            self.place_primitive(primitive, true);
            return;
        };
        // The untransformed primitive is the operation: replayed in another
        // transform, it lands where that one puts it, even if hidden now.
        self.place_primitive(transform.apply(&primitive), false);
        self.paint_operations
            .push(PaintOperation::Primitive(primitive));
    }

    fn place_primitive(&mut self, mut primitive: Primitive, record: bool) {
        let clipped_bounds = primitive
            .bounds()
            .intersect(&primitive.content_mask().bounds);

        if clipped_bounds.is_empty() {
            return;
        }

        let order = self
            .layer_stack
            .last()
            .copied()
            .unwrap_or_else(|| self.primitive_bounds.insert(clipped_bounds));
        match &mut primitive {
            Primitive::Shadow(shadow) => {
                shadow.order = order;
                self.shadows.push(*shadow);
            }
            Primitive::Quad(quad) => {
                quad.order = order;
                self.quads.push(*quad);
            }
            Primitive::Path(path) => {
                path.order = order;
                path.id = PathId(self.paths.len());
                self.paths.push(path.clone());
            }
            Primitive::Underline(underline) => {
                underline.order = order;
                self.underlines.push(*underline);
            }
            Primitive::MonochromeSprite(sprite) => {
                sprite.order = order;
                self.monochrome_sprites.push(*sprite);
            }
            Primitive::SubpixelSprite(sprite) => {
                sprite.order = order;
                self.subpixel_sprites.push(*sprite);
            }
            Primitive::PolychromeSprite(sprite) => {
                sprite.order = order;
                self.polychrome_sprites.push(*sprite);
            }
            Primitive::Surface(surface) => {
                surface.order = order;
                self.surfaces.push(surface.clone());
            }
        }
        if record {
            self.paint_operations
                .push(PaintOperation::Primitive(primitive));
        }
    }

    pub fn replay(&mut self, range: Range<usize>, prev_scene: &Scene) {
        for operation in &prev_scene.paint_operations[range] {
            match operation {
                PaintOperation::Primitive(primitive) => self.insert_primitive(primitive.clone()),
                PaintOperation::StartLayer(bounds) => self.push_layer(*bounds),
                PaintOperation::EndLayer => self.pop_layer(),
                PaintOperation::PushTransform(transform) => self.push_transform(*transform),
                PaintOperation::PushLiveTransform {
                    transform,
                    anchor,
                    scale_factor,
                } => self.push_live_transform(transform.clone(), *anchor, *scale_factor),
                PaintOperation::PopTransform => self.pop_transform(),
            }
        }
    }

    pub fn finish(&mut self) {
        self.shadows.sort_by_key(|shadow| shadow.order);
        self.quads.sort_by_key(|quad| quad.order);
        self.paths.sort_by_key(|path| path.order);
        self.underlines.sort_by_key(|underline| underline.order);
        self.monochrome_sprites
            .sort_by_key(|sprite| (sprite.order, sprite.tile.tile_id));
        self.subpixel_sprites
            .sort_by_key(|sprite| (sprite.order, sprite.tile.tile_id));
        self.polychrome_sprites
            .sort_by_key(|sprite| (sprite.order, sprite.tile.tile_id));
        self.surfaces.sort_by_key(|surface| surface.order);
    }

    #[cfg_attr(
        all(
            any(target_os = "linux", target_os = "freebsd"),
            not(any(feature = "x11", feature = "wayland"))
        ),
        allow(dead_code)
    )]
    pub fn batches(&self) -> impl Iterator<Item = PrimitiveBatch> + '_ {
        BatchIterator {
            shadows_start: 0,
            shadows_iter: self.shadows.iter().peekable(),
            quads_start: 0,
            quads_iter: self.quads.iter().peekable(),
            paths_start: 0,
            paths_iter: self.paths.iter().peekable(),
            underlines_start: 0,
            underlines_iter: self.underlines.iter().peekable(),
            monochrome_sprites_start: 0,
            monochrome_sprites_iter: self.monochrome_sprites.iter().peekable(),
            subpixel_sprites_start: 0,
            subpixel_sprites_iter: self.subpixel_sprites.iter().peekable(),
            polychrome_sprites_start: 0,
            polychrome_sprites_iter: self.polychrome_sprites.iter().peekable(),
            surfaces_start: 0,
            surfaces_iter: self.surfaces.iter().peekable(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Default)]
#[cfg_attr(
    all(
        any(target_os = "linux", target_os = "freebsd"),
        not(any(feature = "x11", feature = "wayland"))
    ),
    allow(dead_code)
)]
pub(crate) enum PrimitiveKind {
    Shadow,
    #[default]
    Quad,
    Path,
    Underline,
    MonochromeSprite,
    SubpixelSprite,
    PolychromeSprite,
    Surface,
}

pub(crate) enum PaintOperation {
    Primitive(Primitive),
    StartLayer(Bounds<ScaledPixels>),
    EndLayer,
    PushTransform(VisualTransform),
    /// A [`LiveTransform`] about `anchor` (device pixels), read again on every replay.
    PushLiveTransform {
        transform: LiveTransform,
        anchor: Point<ScaledPixels>,
        scale_factor: f32,
    },
    PopTransform,
}

/// A translation, scale, opacity and clip for everything painted inside it
/// (see [`crate::Window::with_visual_transform`],
/// [`crate::Window::with_live_transform`] and [`crate::Window::with_visual_clip`]).
/// It only changes what is drawn: layout, hit testing, element bounds and
/// content masks stay as laid out. A device position `p` is drawn at
/// `p * scale + offset`, each axis with its own scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VisualTransform {
    /// Device pixels to move by (after scaling).
    pub offset: Point<ScaledPixels>,
    /// Multiplies horizontal positions and widths about the device origin.
    pub scale_x: f32,
    /// Multiplies vertical positions and heights about the device origin.
    pub scale_y: f32,
    /// Multiplies border widths, corner radii, blur radii and underline
    /// thickness: the scale for a zoom, 1 for a stretch that resizes boxes
    /// without thickening their borders.
    pub lengths: f32,
    /// Multiplies the opacity of everything inside.
    pub opacity: f32,
    /// Device-pixel bounds nothing inside is drawn outside of, placed in the
    /// coordinates around the transform (its own offset and scale do not move it).
    pub clip: Option<Bounds<ScaledPixels>>,
}

impl VisualTransform {
    /// A move by `offset` (device pixels) at `opacity`.
    pub fn moved(offset: Point<ScaledPixels>, opacity: f32) -> Self {
        Self {
            offset,
            scale_x: 1.,
            scale_y: 1.,
            lengths: 1.,
            opacity,
            clip: None,
        }
    }

    fn scaled(&self) -> bool {
        self.scale_x != 1. || self.scale_y != 1.
    }

    /// `inner` inside `self`.
    fn then(self, inner: Self) -> Self {
        Self {
            offset: point(
                self.offset.x + inner.offset.x * self.scale_x,
                self.offset.y + inner.offset.y * self.scale_y,
            ),
            scale_x: self.scale_x * inner.scale_x,
            scale_y: self.scale_y * inner.scale_y,
            lengths: self.lengths * inner.lengths,
            opacity: self.opacity * inner.opacity,
            clip: match (self.clip, inner.clip.map(|clip| self.bounds(clip))) {
                (Some(outer), Some(inner)) => Some(outer.intersect(&inner)),
                (outer, inner) => outer.or(inner),
            },
        }
    }

    fn bounds(&self, bounds: Bounds<ScaledPixels>) -> Bounds<ScaledPixels> {
        Bounds {
            origin: self.point(bounds.origin),
            size: size(
                bounds.size.width * self.scale_x,
                bounds.size.height * self.scale_y,
            ),
        }
    }

    fn point(&self, point: Point<ScaledPixels>) -> Point<ScaledPixels> {
        crate::point(
            point.x * self.scale_x + self.offset.x,
            point.y * self.scale_y + self.offset.y,
        )
    }

    fn length(&self, length: ScaledPixels) -> ScaledPixels {
        length * self.lengths
    }

    fn corners(&self, corners: Corners<ScaledPixels>) -> Corners<ScaledPixels> {
        Corners {
            top_left: self.length(corners.top_left),
            top_right: self.length(corners.top_right),
            bottom_right: self.length(corners.bottom_right),
            bottom_left: self.length(corners.bottom_left),
        }
    }

    fn mask(&self, mask: ContentMask<ScaledPixels>) -> ContentMask<ScaledPixels> {
        let bounds = self.bounds(mask.bounds);
        ContentMask {
            bounds: match self.clip {
                Some(clip) => bounds.intersect(&clip),
                None => bounds,
            },
        }
    }

    /// The primitive as drawn: moved, scaled and faded. Unscaled glyphs and
    /// images move by whole device pixels, where a fresh paint would have
    /// snapped them; scaled ones keep their exact place, so that they do not
    /// jump a pixel at a time while the scale changes.
    fn apply(&self, primitive: &Primitive) -> Primitive {
        let snapped = if self.scaled() {
            *self
        } else {
            VisualTransform {
                offset: point(
                    ScaledPixels(self.offset.x.0.round()),
                    ScaledPixels(self.offset.y.0.round()),
                ),
                ..*self
            }
        };
        let opacity = self.opacity;
        match primitive {
            Primitive::Shadow(shadow) => Primitive::Shadow(Shadow {
                bounds: self.bounds(shadow.bounds),
                content_mask: self.mask(shadow.content_mask),
                element_bounds: self.bounds(shadow.element_bounds),
                corner_radii: self.corners(shadow.corner_radii),
                element_corner_radii: self.corners(shadow.element_corner_radii),
                blur_radius: self.length(shadow.blur_radius),
                color: shadow.color.opacity(opacity),
                ..*shadow
            }),
            Primitive::Quad(quad) => Primitive::Quad(Quad {
                bounds: self.bounds(quad.bounds),
                content_mask: self.mask(quad.content_mask),
                background: quad.background.opacity(opacity),
                border_color: quad.border_color.opacity(opacity),
                corner_radii: self.corners(quad.corner_radii),
                border_widths: Edges {
                    top: self.length(quad.border_widths.top),
                    right: self.length(quad.border_widths.right),
                    bottom: self.length(quad.border_widths.bottom),
                    left: self.length(quad.border_widths.left),
                },
                ..*quad
            }),
            Primitive::Path(path) => {
                let mut path = path.clone();
                path.bounds = self.bounds(path.bounds);
                path.content_mask = self.mask(path.content_mask);
                for vertex in &mut path.vertices {
                    vertex.xy_position = self.point(vertex.xy_position);
                    vertex.content_mask = self.mask(vertex.content_mask.clone());
                }
                path.color = path.color.opacity(opacity);
                Primitive::Path(path)
            }
            Primitive::Underline(underline) => Primitive::Underline(Underline {
                bounds: self.bounds(underline.bounds),
                content_mask: self.mask(underline.content_mask),
                color: underline.color.opacity(opacity),
                thickness: self.length(underline.thickness),
                ..*underline
            }),
            Primitive::MonochromeSprite(sprite) => Primitive::MonochromeSprite(MonochromeSprite {
                bounds: snapped.bounds(sprite.bounds),
                content_mask: self.mask(sprite.content_mask),
                color: sprite.color.opacity(opacity),
                transformation: snapped.matrix(sprite.transformation),
                ..*sprite
            }),
            Primitive::SubpixelSprite(sprite) => Primitive::SubpixelSprite(SubpixelSprite {
                bounds: snapped.bounds(sprite.bounds),
                content_mask: self.mask(sprite.content_mask),
                color: sprite.color.opacity(opacity),
                transformation: snapped.matrix(sprite.transformation),
                ..*sprite
            }),
            Primitive::PolychromeSprite(sprite) => Primitive::PolychromeSprite(PolychromeSprite {
                bounds: snapped.bounds(sprite.bounds),
                content_mask: self.mask(sprite.content_mask),
                corner_radii: self.corners(sprite.corner_radii),
                opacity: sprite.opacity * opacity,
                ..*sprite
            }),
            Primitive::Surface(surface) => {
                let mut surface = surface.clone();
                surface.bounds = snapped.bounds(surface.bounds);
                surface.content_mask = self.mask(surface.content_mask);
                Primitive::Surface(surface)
            }
        }
    }

    /// A sprite's matrix for its transformed bounds. The matrix maps device
    /// positions (`p * rotation_scale + translation`) after the bounds place
    /// them, so its translation takes the scale and absorbs the move it would
    /// rotate: drawn, `p` lands at `(p * rotation_scale + translation) * scale
    /// + offset` (exactly for unrotated sprites, the only ones scaled unevenly).
    fn matrix(&self, matrix: TransformationMatrix) -> TransformationMatrix {
        let [[a, b], [c, d]] = matrix.rotation_scale;
        let (x, y) = (self.offset.x.0, self.offset.y.0);
        let [tx, ty] = matrix.translation;
        TransformationMatrix {
            rotation_scale: matrix.rotation_scale,
            translation: [
                tx * self.scale_x + x - (x * a + y * c),
                ty * self.scale_y + y - (x * b + y * d),
            ],
        }
    }
}

/// What a [`LiveTransform`] draws its contents with, about the anchor it is
/// painted with: a point `p` lands at `anchor + (p - anchor) * scale + offset`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LiveTransformValue {
    /// Logical pixels to move by.
    pub offset: Point<Pixels>,
    /// Multiplies horizontal distances from the anchor.
    pub scale_x: f32,
    /// Multiplies vertical distances from the anchor.
    pub scale_y: f32,
    /// Multiplies border widths, corner radii, blur radii and underline
    /// thickness (see [`VisualTransform::lengths`]).
    pub lengths: f32,
    /// Multiplies the opacity of everything inside.
    pub opacity: f32,
}

impl Default for LiveTransformValue {
    fn default() -> Self {
        Self {
            offset: Point::default(),
            scale_x: 1.,
            scale_y: 1.,
            lengths: 1.,
            opacity: 1.,
        }
    }
}

impl LiveTransformValue {
    /// Scaled by `scale` about the anchor, borders and text included (a zoom), at `opacity`.
    pub fn zoom(scale: f32, opacity: f32) -> Self {
        Self {
            scale_x: scale,
            scale_y: scale,
            lengths: scale,
            opacity,
            ..Self::default()
        }
    }

    /// Whether it draws its contents as laid out.
    pub fn is_identity(&self) -> bool {
        *self == Self::default()
    }

    fn visual(&self, anchor: Point<ScaledPixels>, scale_factor: f32) -> VisualTransform {
        let finite = |value: f32| if value.is_finite() { value } else { 1. };
        let (scale_x, scale_y) = (finite(self.scale_x), finite(self.scale_y));
        VisualTransform {
            offset: point(
                anchor.x * (1. - scale_x) + ScaledPixels(self.offset.x.0 * scale_factor),
                anchor.y * (1. - scale_y) + ScaledPixels(self.offset.y.0 * scale_factor),
            ),
            scale_x,
            scale_y,
            lengths: finite(self.lengths),
            opacity: self.opacity.clamp(0., 1.),
            clip: None,
        }
    }
}

/// A visual transform whose value is read whenever the scene is built,
/// including when a cached view painted inside it replays: changing the value
/// and drawing again moves, scales and fades the contents without rendering
/// the view that painted them (see [`crate::Window::with_live_transform`]).
#[derive(Clone, Default)]
pub struct LiveTransform(std::rc::Rc<std::cell::Cell<LiveTransformValue>>);

impl LiveTransform {
    /// A live transform drawing its contents as laid out.
    pub fn new(value: LiveTransformValue) -> Self {
        Self(std::rc::Rc::new(std::cell::Cell::new(value)))
    }

    /// The value the next scene built uses.
    pub fn set(&self, value: LiveTransformValue) {
        self.0.set(value);
    }

    /// The value scenes are built with now.
    pub fn get(&self) -> LiveTransformValue {
        self.0.get()
    }
}

impl Debug for LiveTransform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.get().fmt(f)
    }
}

#[derive(Clone)]
#[expect(missing_docs)]
pub enum Primitive {
    Shadow(Shadow),
    Quad(Quad),
    Path(Path<ScaledPixels>),
    Underline(Underline),
    MonochromeSprite(MonochromeSprite),
    SubpixelSprite(SubpixelSprite),
    PolychromeSprite(PolychromeSprite),
    Surface(PaintSurface),
}

#[expect(missing_docs)]
impl Primitive {
    pub fn bounds(&self) -> &Bounds<ScaledPixels> {
        match self {
            Primitive::Shadow(shadow) => &shadow.bounds,
            Primitive::Quad(quad) => &quad.bounds,
            Primitive::Path(path) => &path.bounds,
            Primitive::Underline(underline) => &underline.bounds,
            Primitive::MonochromeSprite(sprite) => &sprite.bounds,
            Primitive::SubpixelSprite(sprite) => &sprite.bounds,
            Primitive::PolychromeSprite(sprite) => &sprite.bounds,
            Primitive::Surface(surface) => &surface.bounds,
        }
    }

    pub fn content_mask(&self) -> &ContentMask<ScaledPixels> {
        match self {
            Primitive::Shadow(shadow) => &shadow.content_mask,
            Primitive::Quad(quad) => &quad.content_mask,
            Primitive::Path(path) => &path.content_mask,
            Primitive::Underline(underline) => &underline.content_mask,
            Primitive::MonochromeSprite(sprite) => &sprite.content_mask,
            Primitive::SubpixelSprite(sprite) => &sprite.content_mask,
            Primitive::PolychromeSprite(sprite) => &sprite.content_mask,
            Primitive::Surface(surface) => &surface.content_mask,
        }
    }
}

#[cfg_attr(
    all(
        any(target_os = "linux", target_os = "freebsd"),
        not(any(feature = "x11", feature = "wayland"))
    ),
    allow(dead_code)
)]
struct BatchIterator<'a> {
    shadows_start: usize,
    shadows_iter: Peekable<slice::Iter<'a, Shadow>>,
    quads_start: usize,
    quads_iter: Peekable<slice::Iter<'a, Quad>>,
    paths_start: usize,
    paths_iter: Peekable<slice::Iter<'a, Path<ScaledPixels>>>,
    underlines_start: usize,
    underlines_iter: Peekable<slice::Iter<'a, Underline>>,
    monochrome_sprites_start: usize,
    monochrome_sprites_iter: Peekable<slice::Iter<'a, MonochromeSprite>>,
    subpixel_sprites_start: usize,
    subpixel_sprites_iter: Peekable<slice::Iter<'a, SubpixelSprite>>,
    polychrome_sprites_start: usize,
    polychrome_sprites_iter: Peekable<slice::Iter<'a, PolychromeSprite>>,
    surfaces_start: usize,
    surfaces_iter: Peekable<slice::Iter<'a, PaintSurface>>,
}

impl<'a> Iterator for BatchIterator<'a> {
    type Item = PrimitiveBatch;

    fn next(&mut self) -> Option<Self::Item> {
        let mut orders_and_kinds = [
            (
                self.shadows_iter.peek().map(|s| s.order),
                PrimitiveKind::Shadow,
            ),
            (self.quads_iter.peek().map(|q| q.order), PrimitiveKind::Quad),
            (self.paths_iter.peek().map(|q| q.order), PrimitiveKind::Path),
            (
                self.underlines_iter.peek().map(|u| u.order),
                PrimitiveKind::Underline,
            ),
            (
                self.monochrome_sprites_iter.peek().map(|s| s.order),
                PrimitiveKind::MonochromeSprite,
            ),
            (
                self.subpixel_sprites_iter.peek().map(|s| s.order),
                PrimitiveKind::SubpixelSprite,
            ),
            (
                self.polychrome_sprites_iter.peek().map(|s| s.order),
                PrimitiveKind::PolychromeSprite,
            ),
            (
                self.surfaces_iter.peek().map(|s| s.order),
                PrimitiveKind::Surface,
            ),
        ];
        orders_and_kinds.sort_by_key(|(order, kind)| (order.unwrap_or(u32::MAX), *kind));

        let first = orders_and_kinds[0];
        let second = orders_and_kinds[1];
        let (batch_kind, max_order_and_kind) = if first.0.is_some() {
            (first.1, (second.0.unwrap_or(u32::MAX), second.1))
        } else {
            return None;
        };

        match batch_kind {
            PrimitiveKind::Shadow => {
                let shadows_start = self.shadows_start;
                let mut shadows_end = shadows_start + 1;
                self.shadows_iter.next();
                while self
                    .shadows_iter
                    .next_if(|shadow| (shadow.order, batch_kind) < max_order_and_kind)
                    .is_some()
                {
                    shadows_end += 1;
                }
                self.shadows_start = shadows_end;
                Some(PrimitiveBatch::Shadows(shadows_start..shadows_end))
            }
            PrimitiveKind::Quad => {
                let quads_start = self.quads_start;
                let mut quads_end = quads_start + 1;
                self.quads_iter.next();
                while self
                    .quads_iter
                    .next_if(|quad| (quad.order, batch_kind) < max_order_and_kind)
                    .is_some()
                {
                    quads_end += 1;
                }
                self.quads_start = quads_end;
                Some(PrimitiveBatch::Quads(quads_start..quads_end))
            }
            PrimitiveKind::Path => {
                let paths_start = self.paths_start;
                let mut paths_end = paths_start + 1;
                self.paths_iter.next();
                while self
                    .paths_iter
                    .next_if(|path| (path.order, batch_kind) < max_order_and_kind)
                    .is_some()
                {
                    paths_end += 1;
                }
                self.paths_start = paths_end;
                Some(PrimitiveBatch::Paths(paths_start..paths_end))
            }
            PrimitiveKind::Underline => {
                let underlines_start = self.underlines_start;
                let mut underlines_end = underlines_start + 1;
                self.underlines_iter.next();
                while self
                    .underlines_iter
                    .next_if(|underline| (underline.order, batch_kind) < max_order_and_kind)
                    .is_some()
                {
                    underlines_end += 1;
                }
                self.underlines_start = underlines_end;
                Some(PrimitiveBatch::Underlines(underlines_start..underlines_end))
            }
            PrimitiveKind::MonochromeSprite => {
                let texture_id = self.monochrome_sprites_iter.peek().unwrap().tile.texture_id;
                let sprites_start = self.monochrome_sprites_start;
                let mut sprites_end = sprites_start + 1;
                self.monochrome_sprites_iter.next();
                while self
                    .monochrome_sprites_iter
                    .next_if(|sprite| {
                        (sprite.order, batch_kind) < max_order_and_kind
                            && sprite.tile.texture_id == texture_id
                    })
                    .is_some()
                {
                    sprites_end += 1;
                }
                self.monochrome_sprites_start = sprites_end;
                Some(PrimitiveBatch::MonochromeSprites {
                    texture_id,
                    range: sprites_start..sprites_end,
                })
            }
            PrimitiveKind::SubpixelSprite => {
                let texture_id = self.subpixel_sprites_iter.peek().unwrap().tile.texture_id;
                let sprites_start = self.subpixel_sprites_start;
                let mut sprites_end = sprites_start + 1;
                self.subpixel_sprites_iter.next();
                while self
                    .subpixel_sprites_iter
                    .next_if(|sprite| {
                        (sprite.order, batch_kind) < max_order_and_kind
                            && sprite.tile.texture_id == texture_id
                    })
                    .is_some()
                {
                    sprites_end += 1;
                }
                self.subpixel_sprites_start = sprites_end;
                Some(PrimitiveBatch::SubpixelSprites {
                    texture_id,
                    range: sprites_start..sprites_end,
                })
            }
            PrimitiveKind::PolychromeSprite => {
                let texture_id = self.polychrome_sprites_iter.peek().unwrap().tile.texture_id;
                let sprites_start = self.polychrome_sprites_start;
                let mut sprites_end = sprites_start + 1;
                self.polychrome_sprites_iter.next();
                while self
                    .polychrome_sprites_iter
                    .next_if(|sprite| {
                        (sprite.order, batch_kind) < max_order_and_kind
                            && sprite.tile.texture_id == texture_id
                    })
                    .is_some()
                {
                    sprites_end += 1;
                }
                self.polychrome_sprites_start = sprites_end;
                Some(PrimitiveBatch::PolychromeSprites {
                    texture_id,
                    range: sprites_start..sprites_end,
                })
            }
            PrimitiveKind::Surface => {
                let surfaces_start = self.surfaces_start;
                let mut surfaces_end = surfaces_start + 1;
                self.surfaces_iter.next();
                while self
                    .surfaces_iter
                    .next_if(|surface| (surface.order, batch_kind) < max_order_and_kind)
                    .is_some()
                {
                    surfaces_end += 1;
                }
                self.surfaces_start = surfaces_end;
                Some(PrimitiveBatch::Surfaces(surfaces_start..surfaces_end))
            }
        }
    }
}

#[derive(Debug)]
#[cfg_attr(
    all(
        any(target_os = "linux", target_os = "freebsd"),
        not(any(feature = "x11", feature = "wayland"))
    ),
    allow(dead_code)
)]
#[allow(missing_docs)]
pub enum PrimitiveBatch {
    Shadows(Range<usize>),
    Quads(Range<usize>),
    Paths(Range<usize>),
    Underlines(Range<usize>),
    MonochromeSprites {
        texture_id: AtlasTextureId,
        range: Range<usize>,
    },
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    SubpixelSprites {
        texture_id: AtlasTextureId,
        range: Range<usize>,
    },
    PolychromeSprites {
        texture_id: AtlasTextureId,
        range: Range<usize>,
    },
    Surfaces(Range<usize>),
}

impl PrimitiveBatch {
    #[expect(missing_docs)]
    pub fn label(&self) -> String {
        match self {
            Self::Shadows(range) => format!("shadows ({})", range.len()),
            Self::Quads(range) => format!("quads ({})", range.len()),
            Self::Paths(range) => format!("paths ({})", range.len()),
            Self::Underlines(range) => format!("underlines ({})", range.len()),
            Self::MonochromeSprites { texture_id, range } => {
                format!(
                    "monochrome sprites ({}) on atlas {}",
                    range.len(),
                    texture_id.index
                )
            }
            Self::SubpixelSprites { texture_id, range } => {
                format!(
                    "subpixel sprites ({}) on atlas {}",
                    range.len(),
                    texture_id.index
                )
            }
            Self::PolychromeSprites { texture_id, range } => {
                format!(
                    "polychrome sprites ({}) on atlas {}",
                    range.len(),
                    texture_id.index
                )
            }
            Self::Surfaces(range) => format!("surfaces ({})", range.len()),
        }
    }
}

#[derive(Default, Debug, Copy, Clone)]
#[repr(C)]
#[expect(missing_docs)]
pub struct Quad {
    pub order: DrawOrder,
    pub border_style: BorderStyle,
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    pub background: Background,
    pub border_color: Hsla,
    pub corner_radii: Corners<ScaledPixels>,
    pub border_widths: Edges<ScaledPixels>,
}

impl From<Quad> for Primitive {
    fn from(quad: Quad) -> Self {
        Primitive::Quad(quad)
    }
}

#[derive(Debug, Copy, Clone)]
#[repr(C)]
#[expect(missing_docs)]
pub struct Underline {
    pub order: DrawOrder,
    pub pad: u32, // align to 8 bytes
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    pub color: Hsla,
    pub thickness: ScaledPixels,
    pub wavy: PaddedBool32,
}

impl From<Underline> for Primitive {
    fn from(underline: Underline) -> Self {
        Primitive::Underline(underline)
    }
}

#[derive(Debug, Copy, Clone)]
#[repr(C)]
#[expect(missing_docs)]
pub struct Shadow {
    pub order: DrawOrder,
    pub blur_radius: ScaledPixels,
    pub bounds: Bounds<ScaledPixels>,
    pub corner_radii: Corners<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    pub color: Hsla,
    pub element_bounds: Bounds<ScaledPixels>,
    pub element_corner_radii: Corners<ScaledPixels>,
    /// 0 = drop shadow (rendered outside the element), 1 = inset shadow (rendered inside).
    pub inset: u32,
    pub pad: u32, // align to 8 bytes
}

impl From<Shadow> for Primitive {
    fn from(shadow: Shadow) -> Self {
        Primitive::Shadow(shadow)
    }
}

/// The style of a border.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[repr(C)]
pub enum BorderStyle {
    /// A solid border.
    #[default]
    Solid = 0,
    /// A dashed border.
    Dashed = 1,
}

/// A data type representing a 2 dimensional transformation that can be applied to an element.
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(C)]
pub struct TransformationMatrix {
    /// 2x2 matrix containing rotation and scale,
    /// stored row-major
    pub rotation_scale: [[f32; 2]; 2],
    /// translation vector
    pub translation: [f32; 2],
}

impl Eq for TransformationMatrix {}

impl TransformationMatrix {
    /// The unit matrix, has no effect.
    pub fn unit() -> Self {
        Self {
            rotation_scale: [[1.0, 0.0], [0.0, 1.0]],
            translation: [0.0, 0.0],
        }
    }

    /// Move the origin by a given point
    pub fn translate(mut self, point: Point<ScaledPixels>) -> Self {
        self.compose(Self {
            rotation_scale: [[1.0, 0.0], [0.0, 1.0]],
            translation: [point.x.0, point.y.0],
        })
    }

    /// Clockwise rotation in radians around the origin
    pub fn rotate(self, angle: Radians) -> Self {
        self.compose(Self {
            rotation_scale: [
                [angle.0.cos(), -angle.0.sin()],
                [angle.0.sin(), angle.0.cos()],
            ],
            translation: [0.0, 0.0],
        })
    }

    /// Scale around the origin
    pub fn scale(self, size: Size<f32>) -> Self {
        self.compose(Self {
            rotation_scale: [[size.width, 0.0], [0.0, size.height]],
            translation: [0.0, 0.0],
        })
    }

    /// Perform matrix multiplication with another transformation
    /// to produce a new transformation that is the result of
    /// applying both transformations: first, `other`, then `self`.
    #[inline]
    pub fn compose(self, other: TransformationMatrix) -> TransformationMatrix {
        if other == Self::unit() {
            return self;
        }
        // Perform matrix multiplication
        TransformationMatrix {
            rotation_scale: [
                [
                    self.rotation_scale[0][0] * other.rotation_scale[0][0]
                        + self.rotation_scale[0][1] * other.rotation_scale[1][0],
                    self.rotation_scale[0][0] * other.rotation_scale[0][1]
                        + self.rotation_scale[0][1] * other.rotation_scale[1][1],
                ],
                [
                    self.rotation_scale[1][0] * other.rotation_scale[0][0]
                        + self.rotation_scale[1][1] * other.rotation_scale[1][0],
                    self.rotation_scale[1][0] * other.rotation_scale[0][1]
                        + self.rotation_scale[1][1] * other.rotation_scale[1][1],
                ],
            ],
            translation: [
                self.translation[0]
                    + self.rotation_scale[0][0] * other.translation[0]
                    + self.rotation_scale[0][1] * other.translation[1],
                self.translation[1]
                    + self.rotation_scale[1][0] * other.translation[0]
                    + self.rotation_scale[1][1] * other.translation[1],
            ],
        }
    }

    /// Apply transformation to a point, mainly useful for debugging
    pub fn apply(&self, point: Point<Pixels>) -> Point<Pixels> {
        let input = [point.x.0, point.y.0];
        let mut output = self.translation;
        for (i, output_cell) in output.iter_mut().enumerate() {
            for (k, input_cell) in input.iter().enumerate() {
                *output_cell += self.rotation_scale[i][k] * *input_cell;
            }
        }
        Point::new(output[0].into(), output[1].into())
    }
}

impl Default for TransformationMatrix {
    fn default() -> Self {
        Self::unit()
    }
}

#[derive(Copy, Clone, Debug)]
#[repr(C)]
#[expect(missing_docs)]
pub struct MonochromeSprite {
    pub order: DrawOrder,
    pub pad: u32,
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    pub color: Hsla,
    pub tile: AtlasTile,
    pub transformation: TransformationMatrix,
}

impl From<MonochromeSprite> for Primitive {
    fn from(sprite: MonochromeSprite) -> Self {
        Primitive::MonochromeSprite(sprite)
    }
}

#[derive(Copy, Clone, Debug)]
#[repr(C)]
#[expect(missing_docs)]
pub struct SubpixelSprite {
    pub order: DrawOrder,
    pub pad: u32, // align to 8 bytes
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    pub color: Hsla,
    pub tile: AtlasTile,
    pub transformation: TransformationMatrix,
}

impl From<SubpixelSprite> for Primitive {
    fn from(sprite: SubpixelSprite) -> Self {
        Primitive::SubpixelSprite(sprite)
    }
}

#[derive(Copy, Clone, Debug)]
#[repr(C)]
#[expect(missing_docs)]
pub struct PolychromeSprite {
    pub order: DrawOrder,
    /// Argus: an area-filter mode or 0 (the field keeps its name and layout,
    /// so renderers that ignore it treat it as padding).
    pub pad: u32,
    pub grayscale: PaddedBool32,
    pub opacity: f32,
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    pub corner_radii: Corners<ScaledPixels>,
    pub tile: AtlasTile,
}

impl PolychromeSprite {
    /// Sample the image by the area each pixel covers, with premultiplied
    /// texels, instead of bilinearly: for rasters drawn at fractional positions
    /// from more texels than pixels (Windows renderer).
    pub const AREA_FILTER: u32 = 1;

    /// Area-filter an image whose RGB channels are already multiplied by alpha.
    /// The Windows renderer combines adjacent texels with hardware linear sampling.
    pub const PREMULTIPLIED_AREA_FILTER: u32 = 2;
}

impl From<PolychromeSprite> for Primitive {
    fn from(sprite: PolychromeSprite) -> Self {
        Primitive::PolychromeSprite(sprite)
    }
}

#[derive(Clone, Debug)]
#[allow(missing_docs)]
pub struct PaintSurface {
    pub order: DrawOrder,
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub image_buffer: objc2_core_foundation::CFRetained<objc2_core_video::CVPixelBuffer>,
}

impl From<PaintSurface> for Primitive {
    fn from(surface: PaintSurface) -> Self {
        Primitive::Surface(surface)
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[expect(missing_docs)]
pub struct PathId(pub usize);

/// A line made up of a series of vertices and control points.
#[derive(Clone, Debug)]
#[expect(missing_docs)]
pub struct Path<P: Clone + Debug + Default + PartialEq> {
    pub id: PathId,
    pub order: DrawOrder,
    pub bounds: Bounds<P>,
    pub content_mask: ContentMask<P>,
    pub vertices: Vec<PathVertex<P>>,
    pub color: Background,
    start: Point<P>,
    current: Point<P>,
    contour_count: usize,
}

impl Path<Pixels> {
    /// Create a new path with the given starting point.
    pub fn new(start: Point<Pixels>) -> Self {
        Self {
            id: PathId(0),
            order: DrawOrder::default(),
            vertices: Vec::new(),
            start,
            current: start,
            bounds: Bounds {
                origin: start,
                size: Default::default(),
            },
            content_mask: Default::default(),
            color: Default::default(),
            contour_count: 0,
        }
    }

    /// Scale this path by the given factor.
    pub fn scale(&self, factor: f32) -> Path<ScaledPixels> {
        Path {
            id: self.id,
            order: self.order,
            bounds: self.bounds.scale(factor),
            content_mask: self.content_mask.scale(factor),
            vertices: self
                .vertices
                .iter()
                .map(|vertex| vertex.scale(factor))
                .collect(),
            start: self.start.map(|start| start.scale(factor)),
            current: self.current.scale(factor),
            contour_count: self.contour_count,
            color: self.color,
        }
    }

    /// Move the start, current point to the given point.
    pub fn move_to(&mut self, to: Point<Pixels>) {
        self.contour_count += 1;
        self.start = to;
        self.current = to;
    }

    /// Draw a straight line from the current point to the given point.
    pub fn line_to(&mut self, to: Point<Pixels>) {
        self.contour_count += 1;
        if self.contour_count > 1 {
            self.push_triangle(
                (self.start, self.current, to),
                (point(0., 1.), point(0., 1.), point(0., 1.)),
            );
        }
        self.current = to;
    }

    /// Draw a curve from the current point to the given point, using the given control point.
    pub fn curve_to(&mut self, to: Point<Pixels>, ctrl: Point<Pixels>) {
        self.contour_count += 1;
        if self.contour_count > 1 {
            self.push_triangle(
                (self.start, self.current, to),
                (point(0., 1.), point(0., 1.), point(0., 1.)),
            );
        }

        self.push_triangle(
            (self.current, ctrl, to),
            (point(0., 0.), point(0.5, 0.), point(1., 1.)),
        );
        self.current = to;
    }

    /// Push a triangle to the Path.
    pub fn push_triangle(
        &mut self,
        xy: (Point<Pixels>, Point<Pixels>, Point<Pixels>),
        st: (Point<f32>, Point<f32>, Point<f32>),
    ) {
        self.bounds = self
            .bounds
            .union(&Bounds {
                origin: xy.0,
                size: Default::default(),
            })
            .union(&Bounds {
                origin: xy.1,
                size: Default::default(),
            })
            .union(&Bounds {
                origin: xy.2,
                size: Default::default(),
            });

        self.vertices.push(PathVertex {
            xy_position: xy.0,
            st_position: st.0,
            content_mask: Default::default(),
        });
        self.vertices.push(PathVertex {
            xy_position: xy.1,
            st_position: st.1,
            content_mask: Default::default(),
        });
        self.vertices.push(PathVertex {
            xy_position: xy.2,
            st_position: st.2,
            content_mask: Default::default(),
        });
    }
}

impl<T> Path<T>
where
    T: Clone + Debug + Default + PartialEq + PartialOrd + Add<T, Output = T> + Sub<Output = T>,
{
    #[allow(unused)]
    #[expect(missing_docs)]
    pub fn clipped_bounds(&self) -> Bounds<T> {
        self.bounds.intersect(&self.content_mask.bounds)
    }
}

impl From<Path<ScaledPixels>> for Primitive {
    fn from(path: Path<ScaledPixels>) -> Self {
        Primitive::Path(path)
    }
}

#[derive(Clone, Debug)]
#[repr(C)]
#[expect(missing_docs)]
pub struct PathVertex<P: Clone + Debug + Default + PartialEq> {
    pub xy_position: Point<P>,
    pub st_position: Point<f32>,
    pub content_mask: ContentMask<P>,
}

#[expect(missing_docs)]
impl PathVertex<Pixels> {
    pub fn scale(&self, factor: f32) -> PathVertex<ScaledPixels> {
        PathVertex {
            xy_position: self.xy_position.scale(factor),
            st_position: self.st_position,
            content_mask: self.content_mask.scale(factor),
        }
    }
}
